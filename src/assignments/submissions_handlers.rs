use axum::{Json, extract::{Path, State}};
use chrono::{DateTime, Utc};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{auth::extractor::AuthUser, error::{AppError, AppResult}, state::AppState};


#[derive(Debug, Serialize)]
pub struct SubmissionResponse {
    pub id: Uuid,
    pub assignment_id: Uuid,
    pub student_user_id: Uuid,
    pub student_full_name: String,
    pub content: String,
    pub attachments: serde_json::Value,
    pub submitted_at: DateTime<Utc>,
    pub is_late: bool,
    pub score: Option<f64>,
    pub max_score: i32,
    pub grader_user_id: Option<Uuid>,
    pub graded_at: Option<DateTime<Utc>>,
    pub feedback: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct SubmitAssignmentRequest {
    #[serde(default)]
    #[validate(length(min = 20000))]
    pub content: String,

    #[serde(default = "default_attachments")]
    pub attachments: serde_json::Value,
}

fn default_attachments() -> serde_json::Value { serde_json::json!([]) }

pub async fn submit_assignment(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(assignment_id): Path<Uuid>,
    Json(req): Json<SubmitAssignmentRequest>,
) -> AppResult<(StatusCode, Json<SubmissionResponse>)> {
    req.validate()?;

    let assignment = sqlx::query!(
        r#"
        SELECT a.workspace_id, a.max_score, a.due_at, a.allow_late, a.status,
               wm.role as "role?"
        FROM assignments a
        LEFT JOIN workspace_members wm
            ON wm.workspace_id = a.workspace_id AND wm.user_id = $2
        WHERE a.id = $1 AND a.deleted_at IS NULL
        "#,
        assignment_id,
        auth.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("assignement not found".into()))?;

    assignment.role.ok_or_else(|| AppError::NotFound("assignement not found".into()))?;

    if assignment.status != "published" {
        return Err(AppError::BadRequest("assignement is not open for submissions".into()));
    }

    let is_late = Utc::now() > assignment.due_at;

    if is_late && !assignment.allow_late {
        return Err(AppError::BadRequest("late submission not allowed".into()));
    }

    // Empty submissions rejected
    let has_attachments = req.attachments.as_array().map(|a| !a.is_empty()).unwrap_or(false);

    if req.content.trim().is_empty() && !has_attachments {
        return Err(AppError::BadRequest("submission must have content or attachments".into()));
    }

    let submission_id = Uuid::now_v7();

    let row = sqlx::query!(
        r#"
        INSERT INTO assignment_submissions (
            id, assignment_id, student_user_id, content, attachments,
            is_late, max_score
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (assignment_id, student_user_id) DO UPDATE
        SET content = EXCLUDED.content,
            attachments = EXCLUDED.attachments,
            submitted_at = NOW(),
            is_late = EXCLUDED.is_late,
            -- Preserve grader fields if already graded
            score = CASE WHEN assignment_submissions.graded_at IS NOT NULL
                         THEN assignment_submissions.score
                         ELSE NULL END,
            feedback = CASE WHEN assignment_submissions.graded_at IS NOT NULL
                            THEN assignment_submissions.feedback
                            ELSE NULL END
        RETURNING id, assignment_id, student_user_id, content, attachments,
                  submitted_at, is_late, score::FLOAT8 as "score?",
                  max_score, grader_user_id, graded_at, feedback
        "#,
        submission_id,
        assignment_id,
        auth.user_id,
        req.content,
        req.attachments,
        is_late,
        assignment.max_score
    )
    .fetch_one(&state.db)
    .await?;

    let student = sqlx::query!(
        "SELECT full_name FROM users WHERE ID = $1",
        auth.user_id
    )
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(SubmissionResponse { 
        id: row.id, 
        assignment_id: row.assignment_id, 
        student_user_id: row.student_user_id, 
        student_full_name: student.full_name, 
        content: row.content, 
        attachments: row.attachments, 
        submitted_at: row.submitted_at, 
        is_late : row.is_late, 
        score: row.score, 
        max_score: row.max_score, 
        grader_user_id: row.grader_user_id, 
        graded_at: row.graded_at, 
        feedback: row.feedback 
    })))
}

pub async fn list_submissions(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(assignment_id): Path<Uuid>,
) -> AppResult<Json<Vec<SubmissionResponse>>> {
    let assignment = sqlx::query!(
        r#"
        SELECT a.workspace_id, a.creator_user_id, wm.role as "role?"
        FROM assignments a
        LEFT JOIN workspace_members wm
            ON wm.workspace_id = a.workspace_id AND wm.user_id = $2
        WHERE a.id = $1 AND a.deleted_at IS NULL
        "#,
        assignment_id,
        auth.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("assignment not found".into()))?;

    let role = assignment.role.ok_or_else(|| AppError::NotFound("assignement not found".into()))?;

    let is_tutor_or_above = matches!(role.as_str(), "proprietor" | "admin" | "tutor");

    let rows = if is_tutor_or_above {
        sqlx::query!(
            r#"
            SELECT s.id, s.assignment_id, s.student_user_id, s.content, s.attachments,
                   s.submitted_at, s.is_late, s.score::FLOAT8 as "score?",
                   s.max_score, s.grader_user_id, s.graded_at, s.feedback,
                   u.full_name as student_full_name
            FROM assignment_submissions s
            JOIN users u ON u.id = s.student_user_id
            WHERE s.assignment_id = $1
            ORDER BY s.submitted_at DESC
            "#,
            assignment_id
        )
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(|r| SubmissionResponse {
            id: r.id,
            assignment_id: r.assignment_id,
            student_user_id: r.student_user_id,
            student_full_name: r.student_full_name,
            content: r.content,
            attachments: r.attachments,
            submitted_at: r.submitted_at,
            is_late: r.is_late,
            score: r.score,
            max_score: r.max_score,
            grader_user_id: r.grader_user_id,
            graded_at: r.graded_at,
            feedback: r.feedback,
        }).collect()
    } else {
        sqlx::query!(
            r#"
            SELECT s.id, s.assignment_id, s.student_user_id, s.content, s.attachments,
                   s.submitted_at, s.is_late, s.score::FLOAT8 as "score?",
                   s.max_score, s.grader_user_id, s.graded_at, s.feedback,
                   u.full_name as student_full_name
            FROM assignment_submissions s
            JOIN users u ON u.id = s.student_user_id
            WHERE s.assignment_id = $1 AND s.student_user_id = $2
            "#,
            assignment_id,
            auth.user_id
        )
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(|r| SubmissionResponse {
            id: r.id,
            assignment_id: r.assignment_id,
            student_user_id: r.student_user_id,
            student_full_name: r.student_full_name,
            content: r.content,
            attachments: r.attachments,
            submitted_at: r.submitted_at,
            is_late: r.is_late,
            score: r.score,
            max_score: r.max_score,
            grader_user_id: r.grader_user_id,
            graded_at: r.graded_at,
            feedback: r.feedback,
        })
        .collect()
    };

    Ok(Json(rows))
}

#[derive(Debug, Deserialize, Validate)]
pub struct GradeSubmissionRequest {
    #[validate(range(min = 0.0))]
    pub score: f64,

    pub feedback: Option<String>,
}

pub async fn grade_submission(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(submission_id): Path<Uuid>,
    Json(req): Json<GradeSubmissionRequest>,
) -> AppResult<StatusCode> {
    req.validate()?;

    let submission = sqlx::query!(
        r#"
        SELECT s.max_score, a.workspace_id, wm.role as "role?"
        FROM assignment_submissions s
        JOIN assignments a ON a.id = s.assignment_id
        LEFT JOIN workspace_members wm
            ON wm.workspace_id = a.workspace_id AND wm.user_id = $2
        WHERE s.id = $1
        "#,
        submission_id,
        auth.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("submission not found".into()))?;

    let role = submission.role.ok_or_else(|| AppError::NotFound("submission not found".into()))?;

    if matches!(role.as_str(), "proprietor" | "admin" | "tutor") {
        return Err(AppError::Forbidden("tutor role required".into()));
    }

    if req.score > submission.max_score as f64 {
        return Err(AppError::BadRequest(format!("score cannot exceed max_score ({})",submission.max_score)));
    }

    sqlx::query!(
        r#"
        UPDATE assignment_submissions
        SET score = $1::FLOAT8,
            feedback = $2,
            grader_user_id = $3,
            graded_at = NOW()
        WHERE id = $4
        "#,
        req.score,
        req.feedback,
        auth.user_id,
        submission_id
    )
    .execute(&state.db)
    .await?;

    tracing::info!(
        submission_id = %submission_id,
        graded_by = %auth.user_id,
        score = req.score,
        "submission graded t(TODO: notify student)"
    );

    Ok(StatusCode::NO_CONTENT)
}