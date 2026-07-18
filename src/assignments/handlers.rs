use axum::{Json, extract::{Path, State}};
use chrono::{DateTime, Utc};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{auth::extractor::AuthUser, error::{AppError, AppResult}, state::AppState, workspaces::{extractor::WorkspaceContext}};

#[derive(Debug, Serialize)]
pub struct AssignmentResponse {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub channel_id: Uuid,
    pub creator_user_id: Uuid,
    pub creator_full_name: String,
    pub title: String,
    pub description: String,
    pub attachments: serde_json::Value,
    pub max_score: i32,
    pub due_at: DateTime<Utc>,
    pub allow_late: bool,
    pub late_penalty_percent: i32,
    pub status: String,
    pub submission_count: i32,
    pub published_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateAssignmentRequest {
    pub channel_id: Uuid,

    #[validate(length(min = 2, max = 200))]
    pub title: String,

    #[serde(default)]
    pub description: String,

    #[serde(default = "default_attachments")]
    pub attachments: serde_json::Value,

    #[validate(range(min = 1))]
    pub max_score: i32,

    pub due_at: DateTime<Utc>,

    #[serde(default = "default_true")]
    pub allow_late: bool,

    #[serde(default)]
    #[validate(range(min = 0, max = 100))]
    pub late_penalty_percent: i32,
}

fn default_attachments() -> serde_json::Value { serde_json::json!([]) }
fn default_true() -> bool { true }

pub async fn create_assignment(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Json(req): Json<CreateAssignmentRequest>,
) -> AppResult<(StatusCode, Json<AssignmentResponse>)> {
    req.validate()?;

    if !ctx.role.is_tutor_or_above() {
        return Err(AppError::Forbidden("tutor role required".into()));
    }

    if req.due_at <= Utc::now() {
        return Err(AppError::BadRequest("due date must be in the future".into()));
    }

    // Validate channel belongs to workspace and isn't archived
    let channel_exists = sqlx::query!(
        r#"
        SELECT 1 AS "exists!" FROM channels
        WHERE id = $1 AND workspace_id = $2 AND archived = FALSE
        "#,
        req.channel_id,
        ctx.workspace_id
    )
    .fetch_optional(&state.db)
    .await?;

    if channel_exists.is_none() {
        return Err(AppError::BadRequest("channel not found in workspace".into()));
    }

    let assignment_id = Uuid::now_v7();

    let row = sqlx::query!(
        r#"
        INSERT INTO assignments (
            id, workspace_id, channel_id, creator_user_id,
            title, description, attachments, max_score,
            due_at, allow_late, late_penalty_percent
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
        RETURNING id, workspace_id, channel_id, creator_user_id, title, description,
                  attachments, max_score, due_at, allow_late, late_penalty_percent,
                  status, submission_count, published_at, created_at
        "#,
        assignment_id,
        ctx.workspace_id,
        req.channel_id,
        ctx.user_id,
        req.title,
        req.description,
        req.attachments,
        req.max_score,
        req.due_at,
        req.allow_late,
        req.late_penalty_percent
    )
    .fetch_one(&state.db)
    .await?;

    let creator = sqlx::query!(
        "SELECT full_name FROM users WHERE id = $1",
        ctx.user_id
    )
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(AssignmentResponse { 
        id: row.id, 
        workspace_id: row.workspace_id, 
        channel_id: row.channel_id, 
        creator_user_id: row.creator_user_id, 
        creator_full_name: creator.full_name,
        title: row.title, 
        description: row.description, 
        attachments: row.attachments, 
        max_score: row.max_score, 
        due_at: row.due_at, 
        allow_late: row.allow_late, 
        late_penalty_percent: row.late_penalty_percent, 
        status: row.status, 
        submission_count: row.submission_count, 
        published_at: row.published_at, 
        created_at: row.created_at, 
    })))
}

pub async fn publish_assignments(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(assignment_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    let assignment = sqlx::query!(
        r#"
        SELECT a.creator_user_id, a.status, a.workspace_id, wm.role as "role?"
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

    let role = assignment.role.ok_or_else(|| AppError::NotFound("assignment not found".into()))?;

    let is_creator = assignment.creator_user_id == auth.user_id;
    let is_admin = matches!(role.as_str(), "proprietor" | "admin");

    if !is_creator && !is_admin {
        return Err(AppError::Forbidden("creator or admin only".into()));
    }

    if assignment.status != "draft" {
        return Err(AppError::BadRequest("assignment is not in draft status".into()));
    }

    sqlx::query!(
        r#"
        UPDATE assignments
        SET status = 'published', published_at = NOW()
        WHERE id = $1
        "#,
        assignment_id
    )
    .execute(&state.db)
    .await?;

    // TODO: enqueue push notifications to channel members
    tracing::info!(
        assignment_id = %assignment_id,
        "assignment published (TODO: notify channel members)"
    );

    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_assignments(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
) -> AppResult<Json<Vec<AssignmentResponse>>> {
    let show_drafts = ctx.role.is_tutor_or_above();

    let rows = sqlx::query!(
        r#"
        SELECT a.id, a.workspace_id, a.channel_id, a.creator_user_id, a.title,
               a.description, a.attachments, a.max_score, a.due_at,
               a.allow_late, a.late_penalty_percent, a.status,
               a.submission_count, a.published_at, a.created_at,
               u.full_name as creator_full_name
        FROM assignments a
        JOIN users u ON u.id = a.creator_user_id
        WHERE a.workspace_id = $1
          AND a.deleted_at IS NULL
          AND ($2 OR a.status = 'published')
        ORDER BY a.due_at ASC 
        "#,
        ctx.workspace_id,
        show_drafts
    )
    .fetch_optional(&state.db)
    .await?;

    let assignments = rows.into_iter().map(|r| AssignmentResponse {
        id: r.id,
        workspace_id: r.workspace_id,
        channel_id: r.channel_id,
        creator_user_id: r.creator_user_id,
        creator_full_name: r.creator_full_name,
        title: r.title,
        description: r.description,
        attachments: r.attachments,
        max_score: r.max_score,
        due_at: r.due_at,
        allow_late: r.allow_late,
        late_penalty_percent: r.late_penalty_percent,
        status: r.status,
        submission_count: r.submission_count,
        published_at: r.published_at,
        created_at: r.created_at,
    }).collect();

    Ok(Json(assignments))
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateAssignmentRequest {
    #[validate(length(min = 2, max = 200))]
    pub title: Option<String>,

    pub description: Option<String>,
    pub attachments: Option<serde_json::Value>,

    #[validate(range(min = 1))]
    pub max_score: Option<i32>,

    pub due_at: Option<DateTime<Utc>>,
    pub allow_late: Option<bool>,

    #[validate(range(min = 0, max = 100))]
    pub late_penalty_percent: Option<i32>,
}

pub async fn update_assignment (
    State(state): State<AppState>,
    auth: AuthUser,
    Path(assignment_id): Path<Uuid>,
    Json(req): Json<UpdateAssignmentRequest>,
) -> AppResult<StatusCode> {
    req.validate()?;

    let assignment = sqlx::query!(
        r#"
        SELECT a.creator_user_id, wm.role as "role?"
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
    let is_creator = assignment.creator_user_id == auth.user_id;
    let is_admin = matches!(role.as_str(), "Proprietor" | "admin");

    if !is_creator || !is_admin {
        return Err(AppError::Forbidden("creator or admin only".into()));
    }

    sqlx::query!(
        r#"
        UPDATE assignments SET
            title = COALESCE($1, title),
            description = COALESCE($2, description),
            attachments = COALESCE($3, attachments),
            max_score = COALESCE($4, max_score),
            due_at = COALESCE($5, due_at),
            allow_late = COALESCE($6, allow_late),
            late_penalty_percent = COALESCE($7, late_penalty_percent)
        WHERE id = $8 
        "#,
        req.title,
        req.description,
        req.attachments,
        req.max_score,
        req.due_at,
        req.allow_late,
        req.late_penalty_percent,
        assignment_id
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}


pub async fn delete_assignment(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(assignment_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    let assignment = sqlx::query!(
        r#"
        SELECT a.creator_user_id, wm.role as "role?"
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

    let role = assignment.role.ok_or_else(|| AppError::NotFound("assignment not found".into()))?;

    let is_creator = assignment.creator_user_id == auth.user_id;
    let is_admin = matches!(role.as_str(), "proprietor" | "admin");

    if !is_admin && !is_creator {
        return Err(AppError::Forbidden("creator or admin only".into()));
    }

    sqlx::query!(
        "UPDATE assignments SET DELETED_AT = NOW() WHERE id = $1",
        assignment_id
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}
