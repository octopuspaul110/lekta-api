use axum::{Json, extract::{Path, State}};
use chrono::{DateTime, Utc};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{auth::extractor::AuthUser, error::{AppError::{self}, AppResult}, exams::selection::{SelectionCriteria, validate_criteria}, state::AppState, workspaces::extractor::WorkspaceContext};

#[derive(Debug, Serialize)]
pub struct ExamResponse {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub selection_criteria: serde_json::Value,
    pub duration_minutes: i32,
    pub total_marks: i32,
    pub pass_mark_percentage: i32,
    pub scheduled_start_at: Option<DateTime<Utc>>,
    pub scheduled_end_at: Option<DateTime<Utc>>,
    pub eligibility: serde_json::Value,
    pub allow_retakes: bool,
    pub max_attempts: i32,
    pub randomize_questions: bool,
    pub randomize_options: bool,
    pub show_results_immediately: bool,
    pub status: String,
    pub attempt_count: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateExamRequest {
    #[validate(length(min = 2, max = 200))]
    pub title: String,

    pub description: Option<String>,

    pub selection_criteria: SelectionCriteria,

    #[validate(range(min = 5, max = 480))]
    pub duration_minutes: i32,

    #[validate(range(min = 0, max = 100))]
    #[serde(default = "default_pass_mark")]
    pub pass_mark_percent: i32,

    pub scheduled_start_at: Option<DateTime<Utc>>,
    pub scheduled_end_at: Option<DateTime<Utc>>,

    #[serde(default = "default_eligibility")]
    pub eligibility: serde_json::Value,

    #[serde(default)]
    pub allow_retakes: bool,

    #[serde(default = "default_max_attempts")]
    #[validate(range(min = 1, max = 10))]
    pub max_attempts: i32,

    #[serde(default = "default_true")]
    pub randomize_questions: bool,

    #[serde(default = "default_true")]
    pub randomize_options: bool,

    #[serde(default = "default_true")]
    pub show_results_immediately: bool,
}

fn default_pass_mark() -> i32 {50}
fn default_max_attempts() -> i32 {1}
fn default_true() -> bool { true }
fn default_eligibility() -> serde_json::Value {
    serde_json::json!({"type": "all_Students"})
}

pub async fn create_exam(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Json(req): Json<CreateExamRequest>,
) -> AppResult<(StatusCode, Json<ExamResponse>)> {
    req.validate()?;

    if !ctx.role.is_tutor_or_above() {
        return Err(AppError::Forbidden("tutor role required".into()));
    }

    // Validate schedule times
    if let (Some(start), Some(end)) = (req.scheduled_start_at, req.scheduled_end_at) {
        if end <= start {
            return Err(AppError::BadRequest("scheduled_end_at must be after scheduled_start_at".into()));
        }
    }

    // Validate selection criteria produces enough questions
    let total_marks = validate_criteria(&state.db, &req.selection_criteria, ctx.workspace_id).await?;

    if total_marks == 0 {
        return Err(AppError::BadRequest("resolved questions have zero total marks".into()));
    }

    let exam_id = Uuid::now_v7();
    let criteria_json = serde_json::to_value(&req.selection_criteria)?;

    let row = sqlx::query!(
        r#"
        INSERT INTO exams (
            id, workspace_id, creator_user_id, title, description,
            selection_criteria, duration_minutes, total_marks, pass_mark_percent,
            scheduled_start_at, scheduled_ends_at, eligibility,
            allow_retakes, max_attempts, randomize_questions, randomize_options,
            show_results_immediately
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
        RETURNING id, workspace_id, title, description, selection_criteria,
                  duration_minutes, total_marks, pass_mark_percent,
                  scheduled_start_at, scheduled_ends_at as "scheduled_end_at", eligibility,
                  allow_retakes, max_attempts, randomize_questions, randomize_options,
                  show_results_immediately, status, attempt_count, created_at
        "#,
        exam_id,
        ctx.workspace_id,
        ctx.user_id,
        req.title,
        req.description,
        criteria_json,
        req.duration_minutes,
        total_marks as i32,
        req.pass_mark_percent,
        req.scheduled_start_at,
        req.scheduled_end_at,
        req.eligibility,
        req.allow_retakes,
        req.max_attempts,
        req.randomize_questions,
        req.randomize_options,
        req.show_results_immediately
    )
    .fetch_one(&state.db)
    .await?;

    
    Ok((StatusCode::CREATED, Json(ExamResponse { 
        id: row.id, 
        workspace_id: row.workspace_id, 
        title: row.title, 
        description: row.description, 
        selection_criteria: row.selection_criteria, 
        duration_minutes: row.duration_minutes, 
        total_marks: row.total_marks, 
        pass_mark_percentage: row.pass_mark_percent, 
        scheduled_start_at: row.scheduled_start_at, 
        scheduled_end_at: row.scheduled_end_at, 
        eligibility: row.eligibility, 
        allow_retakes: row.allow_retakes, 
        max_attempts: row.max_attempts, 
        randomize_questions: row.randomize_questions, 
        randomize_options: row.randomize_options, 
        show_results_immediately: row.show_results_immediately, 
        status: row.status, 
        attempt_count: row.attempt_count, 
        created_at: row.created_at 
    })))
}

pub async fn list_exams(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
) -> AppResult<Json<Vec<ExamResponse>>> {
    let show_drafts = ctx.role.is_tutor_or_above();

    let rows = sqlx::query!(
        r#"
        SELECT id, workspace_id, title, description, selection_criteria,
               duration_minutes, total_marks, pass_mark_percent,
               scheduled_start_at, scheduled_ends_at as "scheduled_end_at", eligibility,
               allow_retakes, max_attempts, randomize_questions, randomize_options,
               show_results_immediately, status, attempt_count, created_at
        FROM exams
        WHERE workspace_id = $1
          AND deleted_at IS NULL
          AND ($2 OR status != 'draft')
        ORDER BY scheduled_start_at ASC NULLS LAST, created_at DESC
        "#,
        ctx.workspace_id,
        show_drafts
        
    )
    .fetch_all(&state.db)
    .await?;

    let exams = rows.into_iter().map(|r| ExamResponse {
        id: r.id,
        workspace_id: r.workspace_id,
        title: r.title,
        description: r.description,
        selection_criteria: r.selection_criteria,
        duration_minutes: r.duration_minutes,
        total_marks: r.total_marks,
        pass_mark_percentage: r.pass_mark_percent,
        scheduled_start_at: r.scheduled_start_at,
        scheduled_end_at: r.scheduled_end_at,
        eligibility: r.eligibility,
        allow_retakes: r.allow_retakes,
        max_attempts: r.max_attempts,
        randomize_questions: r.randomize_questions,
        randomize_options: r.randomize_options,
        show_results_immediately: r.show_results_immediately,
        status: r.status,
        attempt_count: r.attempt_count,
        created_at: r.created_at,
    }).collect();

    Ok(Json(exams))
}

pub async fn publish_exams(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(exam_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    let exam = sqlx::query!(
        r#"
        SELECT e.workspace_id, e.creator_user_id, e.status, e.selection_criteria,
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

    let role = exam.role.ok_or_else(|| AppError::NotFound("exam not found".into()))?;

    let is_creator = exam.creator_user_id == auth.user_id;
    let is_admin = matches!(role.as_str(), "proprietor" | "admin");

    if !is_creator && !is_admin {
        return Err(AppError::Forbidden("creator or admin only".into()));
    }

    if !is_creator && !is_admin {
        return Err(AppError::Forbidden("creator or admin only".into()));
    }

    if exam.status != "draft" {
        return Err(AppError::BadRequest("exam is not in draft status".into()));
    }

    // Re-validate criteria (banks might have changed since creation)
    let criteria: SelectionCriteria = serde_json::from_value(exam.selection_criteria)?;
    validate_criteria(&state.db, &criteria, exam.workspace_id).await?;

    sqlx::query!(
        "UPDATE exams SET status = 'scheduled' WHERE id = $1",
        exam_id
    )
    .execute(&state.db)
    .await?;

    tracing::info!(
        exam_id = %exam_id,
        "exam published (TODO: notify eligible students)"
    );

    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_exam(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(exam_id): Path<Uuid>
) -> AppResult<StatusCode> {
    let exam = sqlx::query!(
        r#"
        SELECT e.workspace_id, e.creator_user_id, wm.role as "role?"
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

    let role = exam.role.ok_or_else(|| AppError::NotFound("exam not found".into()))?;

    let is_creator = exam.creator_user_id == auth.user_id;
    let is_admin = matches!(role.as_str(), "proprietor" | "admin");

    if !is_creator && !is_admin {
        return Err(AppError::Forbidden("creator or admin only".into()));
    }

    sqlx::query!(
        "UPDATE exams SET deleted_at = NOW() WHERE id = $1",
        exam_id
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

