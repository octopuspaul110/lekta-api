use axum::{self, Json, extract::{Path, State}};
use chrono::{DateTime, Duration, Utc};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use rand::{SeedableRng, seq::SliceRandom};

use crate::{auth::extractor::AuthUser, error::{AppError, AppResult}, exams::{grading::auto_grade, selection::{SelectionCriteria, resolve_questions}}, state::AppState};

#[derive(Debug, Serialize)]
pub struct ExamAttemptResponse {
    pub id: Uuid,
    pub exam_id: Uuid,
    pub student_user_id: Uuid,
    pub started_at: DateTime<Utc>,
    pub deadline_at: DateTime<Utc>,
    pub submitted_at: Option<DateTime<Utc>>,
    pub status: String,
    pub pass_status: String,
    pub total_score: Option<f64>,
    pub percent_score: Option<f64>,
    pub questions: Vec<ExamQuestionForStudent>,
}

#[derive(Debug, Serialize)]
pub struct ExamQuestionForStudent {
    pub exam_question_id: Uuid,
    pub question_order: i32,
    pub question_text: String,
    pub question_type: String,
    pub options: Option<serde_json::Value>,
    pub marks: i32,
    pub student_answer: Option<serde_json::Value>,
}

pub async fn start_attempt(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(exam_id): Path<Uuid>,
) -> AppResult<(StatusCode, Json<ExamAttemptResponse>)> {
    // Fetch exam + verify accessible
    let exam = sqlx::query!(
        r#"
        SELECT e.workspace_id, e.selection_criteria, e.duration_minutes,
               e.status, e.scheduled_start_at, e.scheduled_ends_at as "scheduled_end_at",
               e.allow_retakes, e.max_attempts,
               e.randomize_questions, e.randomize_options,
               wm.role as "role?"
        FROM exams e
        LEFT JOIN workspace_members wm
            ON wm.workspace_id = e.workspace_id AND wm.user_id = $2
        WHERE e.id = $1 AND e.deleted_at IS NULL
        "#,
        exam_id,
        auth.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("exam not found".into()))?;

    exam.role.ok_or_else(|| AppError::NotFound("exam not found".into()))?;

    if exam.status != "scheduled" && exam.status != "ongoing" {
        return Err(AppError::BadRequest("exam is not open".into()));
    }

    let now = Utc::now();
    if let Some(start) = exam.scheduled_start_at {
        if now < start {
            return Err(AppError::BadRequest("exam has not started yet".into()));
        }
    }
    if let Some(end) = exam.scheduled_end_at {
        if now > end {
            return Err(AppError::BadRequest("exam window has ended".into()));
        }
    }

    // Check for existing in-progress attempt
    let existing = sqlx::query!(
        r#"
        SELECT id FROM exam_attempts
        WHERE exam_id = $1 AND student_user_id = $2 AND status = 'in_progress'
        "#,
        exam_id,
        auth.user_id
    )
    .fetch_optional(&state.db)
    .await?;

    if let Some(existing) = existing {
        // Return existing attempt instead of creating new
        return get_attempt_details(&state, existing.id).await
            .map(|resp| (StatusCode::OK, Json(resp)));
    }

    // Count previous completed attempts
    let previous_count = sqlx::query!(
        r#"
        SELECT COUNT(*) as "count!"
        FROM exam_attempts
        WHERE exam_id = $1 AND student_user_id = $2 AND status != 'voided'
        "#,
        exam_id,
        auth.user_id
    )
    .fetch_one(&state.db)
    .await?;

    if previous_count.count >= exam.max_attempts as i64 {
        return Err(AppError::BadRequest("max attempts reached".into()));
    }

    // Materialize questions
    let criteria: SelectionCriteria = serde_json::from_value(exam.selection_criteria)?;
    let mut questions = resolve_questions(&state.db, &criteria).await?;

    if questions.is_empty() {
        return Err(AppError::Internal("no questions resolved for exam".into()));
    }

    // Shuffle if configured
    let mut rng = rand::rngs::StdRng::from_entropy();

    if exam.randomize_questions {
        questions.shuffle(&mut rng);
    }

    let attempt_id = Uuid::now_v7();
    let deadline_at = now + Duration::minutes(exam.duration_minutes as i64);
    let attempt_number = previous_count.count as i32 + 1;

    let mut tx = state.db.begin().await?;

    sqlx::query!(
        r#"
        INSERT INTO exam_attempts (
            id, exam_id, student_user_id, attempt_number,
            started_at, deadline_at, status
        )
        VALUES ($1, $2, $3, $4, $5, $6, 'in_progress')
        "#,
        attempt_id,
        exam_id,
        auth.user_id,
        attempt_number,
        now,
        deadline_at
    )
    .execute(&mut *tx)
    .await?;

    // Insert exam_questions rows
    for (index, question) in questions.iter().enumerate() {
        let shuffled_options = if exam.randomize_options {
            question.options.as_ref().and_then(|opts| {
                let mut arr: Vec<serde_json::Value> = opts.as_array()?.clone();
                arr.shuffle(&mut rng);
                Some(serde_json::Value::Array(arr))
            })
        } else {
            None
        };

        sqlx::query!(
            r#"
            INSERT INTO exam_questions (
                exam_attempt_id, question_id, question_order, shuffled_options
            )
            VALUES ($1, $2, $3, $4)
            "#,
            attempt_id,
            question.id,
            (index + 1) as i32,
            shuffled_options
        )
        .execute(&mut *tx)
        .await?;
    }

    // Update exam status to ongoing if this is the first attempt
    sqlx::query!(
        r#"
        UPDATE exams SET status = 'ongoing', attempt_count = attempt_count + 1
        WHERE id = $1
        "#,
        exam_id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    let response = get_attempt_details(&state, attempt_id).await?;

    Ok((StatusCode::CREATED, Json(response)))
}

async fn get_attempt_details(state: &AppState, attempt_id: Uuid) -> AppResult<ExamAttemptResponse> {
    let attempt = sqlx::query!(
        r#"
        SELECT id, exam_id, student_user_id, started_at, deadline_at,
               submitted_at, status, pass_status,
               total_score::FLOAT8 as "total_score?",
               percent_score::FLOAT8 as "percent_score?"
        FROM exam_attempts WHERE id = $1
        "#,
        attempt_id
    )
    .fetch_one(&state.db)
    .await?;

    let question_rows = sqlx::query!(
        r#"
        SELECT eq.id as exam_question_id, eq.question_order, eq.shuffled_options,
               q.question_text, q.question_type, q.options, q.marks,
               ea.student_answer
        FROM exam_questions eq
        JOIN questions q ON q.id = eq.question_id
        LEFT JOIN exam_answers ea
            ON ea.exam_question_id = eq.id AND ea.exam_attempt_id = eq.exam_attempt_id
        WHERE eq.exam_attempt_id = $1
        ORDER BY eq.question_order ASC
        "#,
        attempt_id
    )
    .fetch_all(&state.db)
    .await?;

    let questions = question_rows.into_iter().map(|r| ExamQuestionForStudent {
        exam_question_id: r.exam_question_id,
        question_order: r.question_order,
        question_text: r.question_text,
        question_type: r.question_type,
        options: r.options,
        marks: r.marks,
        student_answer: r.student_answer,
    }).collect();

    Ok(ExamAttemptResponse { 
        id: attempt.id, 
        exam_id: attempt.exam_id,
        student_user_id: attempt.student_user_id, 
        started_at: attempt.started_at, 
        deadline_at: attempt.deadline_at, 
        submitted_at: attempt.submitted_at, 
        status: attempt.status, 
        pass_status: attempt.pass_status, 
        total_score: attempt.total_score, 
        percent_score: attempt.percent_score, 
        questions 
    })
}

pub async fn get_attempt(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(attempt_id): Path<Uuid>,
) -> AppResult<Json<ExamAttemptResponse>> {
    let attempt = sqlx::query!(
        r#"
        SELECT ea.student_user_id, e.workspace_id, wm.role as "role?"
        FROM exam_attempts ea
        JOIN exams e ON e.id = ea.exam_id
        LEFT JOIN workspace_members wm
            ON wm.workspace_id = e.workspace_id AND wm.user_id = $2
        WHERE ea.id = $1
        "#,
        attempt_id,
        auth.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("attempt not found".into()))?;

    let role = attempt.role.ok_or_else(|| AppError::NotFound("attempt not found".into()))?;

    let is_owner = attempt.student_user_id == auth.user_id;
    let is_admin = matches!(role.as_str(), "proprietor" | "admin" | "tutor");

    if !is_owner && !is_admin {
        return Err(AppError::NotFound("owner or tutor only".into()));
    }

    let response = get_attempt_details(&state, attempt_id).await?;
    Ok(Json(response))
}

#[derive(Debug, Deserialize)]
pub struct SubmitAnswerRequest {
    pub exam_question_id: Uuid,
    pub student_answer: serde_json::Value,
    pub time_spent_seconds: Option<i32>,
}

pub async fn submit_answer(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(attempt_id): Path<Uuid>,
    Json(req): Json<SubmitAnswerRequest>,
) -> AppResult<StatusCode> {
    let attempt = sqlx::query!(
        r#"
        SELECT student_user_id, deadline_at, status
        FROM exam_attempts WHERE id = $1
        "#,
        attempt_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("attempt not found".into()))?;

    if attempt.student_user_id != auth.user_id {
        return Err(AppError::Forbidden("not your attempt".into()));
    }

    if attempt.status != "in_progress" {
        return Err(AppError::BadRequest("attempt is not in progress".into()));
    }

    if Utc::now() > attempt.deadline_at {
        return Err(AppError::BadRequest("attempt deadline has passed".into()));
    }

    // Verify the exam_question belongs to this attempt
    let question = sqlx::query!(
        r#"
        SELECT eq.id, q.question_type, q.correct_answers as "correct_answer", q.marks
        FROM exam_questions eq
        JOIN questions q ON q.id = eq.question_id
        WHERE eq.id = $1 AND eq.exam_attempt_id = $2
        "#,
        req.exam_question_id,
        attempt_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::BadRequest("question does not belong to this attempt".into()))?;

    // Automatically grade this answer
    let (is_correct, marks) = match auto_grade(
        &question.question_type, 
        &question.correct_answer, 
        &req.student_answer, 
        question.marks
    ) {
        Some((c, m)) => (Some(c), Some(m)),
        None => (None, None),
    };

    sqlx::query!(
        r#"
        INSERT INTO exam_answers (
            exam_attempt_id, exam_question_id, student_answer,
            is_correct, marks_awarded, time_spent_seconds
        )
        VALUES ($1, $2, $3, $4, $5::FLOAT8, $6)
        ON CONFLICT (exam_attempt_id, exam_question_id) DO UPDATE
        SET student_answer = EXCLUDED.student_answer,
            is_correct = EXCLUDED.is_correct,
            marks_awarded = EXCLUDED.marks_awarded,
            time_spent_seconds = EXCLUDED.time_spent_seconds,
            updated_at = NOW()
        "#,
        attempt_id,
        req.exam_question_id,
        req.student_answer,
        is_correct,
        marks,
        req.time_spent_seconds
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn submit_attempt(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(attempt_id): Path<Uuid>,
) -> AppResult<Json<ExamAttemptResponse>> {
    let attempt = sqlx::query!(
        r#"
        SELECT ea.student_user_id, ea.status, e.pass_mark_percent, e.total_marks
        FROM exam_attempts ea
        JOIN exams e ON e.id = ea.exam_id
        WHERE ea.id = $1
        "#,
        attempt_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("attempt not found".into()))?;

    if attempt.student_user_id != auth.user_id {
        return Err(AppError::Forbidden("not your attempt".into()));
    }

    if attempt.status != "in_progress" {
        return Err(AppError::BadRequest("already submitted".into()));
    }

    // Compute total_score from answers
    let scoring = sqlx::query!(
        r#"
        SELECT
            COALESCE(SUM(marks_awarded)::FLOAT8, 0.0) as "total!",
            COUNT(*) FILTER (WHERE marks_awarded IS NULL) as "ungraded!"
        FROM exam_answers
        WHERE exam_attempt_id = $1
        "#,
        attempt_id
    )
    .fetch_one(&state.db)
    .await?;
    
    let percent_score = if attempt.total_marks > 0 {
        (scoring.total / attempt.total_marks as f64) * 100.0
    } else {
        0.0
    };

    let pass_status = if percent_score >= attempt.pass_mark_percent as f64 {
        "passed"
    } else {
        "failed"
    };

    let new_status = if scoring.ungraded == 0 { "graded" } else { "submitted" };

    sqlx::query!(
        r#"
        UPDATE exam_attempts
        SET status = $1,
            submitted_at = NOW(),
            total_score = $2::FLOAT8,
            percent_score = $3::FLOAT8,
            pass_status = $4,
            graded_at = CASE WHEN $1 = 'graded' THEN NOW() ELSE NULL END
        WHERE id = $5
        "#,
        new_status,
        scoring.total,
        percent_score,
        pass_status,
        attempt_id
    )
    .execute(&state.db)
    .await?;

    tracing::info!(
        attempt_id = %attempt_id,
        status = %new_status,
        percent_score = %percent_score,
        "submitted attempt"
    );

    let response = get_attempt_details(&state, attempt_id).await?;
    Ok(Json(response))
}