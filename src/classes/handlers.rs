use axum::{Json, extract::{Path, Query, State}};
use chrono::{DateTime, Utc};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{auth::extractor::AuthUser, error::{AppError, AppResult}, state::AppState, workspaces::extractor::WorkspaceContext};

#[derive(Debug, Serialize)]
pub struct ClassResponse {
    pub id: Uuid,
    pub workspace_id : Uuid,
    pub channel_id: Option<Uuid>,
    pub tutor_user_id: Uuid,
    pub tutor_full_name: String,
    pub title: String,
    pub description: Option<String>,
    pub location: String,
    pub is_online: bool,
    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub recurrence_rule: Option<String>,
    pub status: String,
    pub self_checkin_enabled: bool,
    pub created_at: DateTime<Utc>,
}
#[derive(Debug, Deserialize, Validate)]
pub struct CreateClassRequest {
    pub channel_id: Option<Uuid>,

    #[validate(length(min = 1, max = 500))]
    pub title: String,

    pub description: Option<String>,

    #[validate(length(min = 1, max = 500))]
    pub location: String,

    pub starts_at: DateTime<Utc>,
    pub ends_at: DateTime<Utc>,
    pub tutor_user_id: Option<Uuid>,
    pub recurrence_rule: Option<String>,

    #[serde(default)]
    pub self_checkin_enabled: bool,
}

pub async fn create_class(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Json(req): Json<CreateClassRequest>
) -> AppResult<(StatusCode, Json<ClassResponse>)> {
    req.validate()?;

    if !ctx.role.is_admin_or_above() {
        return Err(AppError::Forbidden("tutor role required".into()));
    }

    if req.ends_at <= req.starts_at {
        return Err(AppError::BadRequest("start time must be before end time".into()));
    }

    // Determine tutor: tutors can only assign themselves; admins can assign anyone
    let tutor_user_id = match req.tutor_user_id {
        Some(uid) => {
            if !ctx.role.is_admin_or_above() && uid != ctx.user_id {
                return Err(AppError::Forbidden("tutors can only create clases for themselves".into()));
            }
            // Verify target is a workspace member with tutor+ role
            let target = sqlx::query!(
                r#"
                SELECT role FROM workspace_members
                WHERE workspace_id = $1 AND user_id = $2 AND status = 'active'
                "#,
                ctx.workspace_id,
                uid
            )
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(|| AppError::NotFound("tutor not found in workspace".into()))?;

            if !matches!(target.role.as_str(), "proprietor" | "admin" | "tutor") {
                return Err(AppError::BadRequest("assigned tutor must have tutor role or higher".into()));
            }
            uid
        }
        None => ctx.user_id,
    };

    // Validate RRULE if provided
    if let Some(rule) = &req.recurrence_rule {
        // Basic sanity check - full parsing via rrule crate
        if rule.is_empty() || !rule.contains("FREQ=") {
            return Err(AppError::BadRequest("invalid recurrence rule".into()));
        }
    }

    if let Some(channel_id) = req.channel_id {
        let exists = sqlx::query!(
            r#"
            SELECT 1 as "exists!" FROM channels
            WHERE id = $1 AND workspace_id = $2 AND archived = FALSE
            "#,
            channel_id,
            ctx.workspace_id
        )
        .fetch_optional(&state.db)
        .await?;

        if exists.is_none() {
            return Err(AppError::BadRequest("channel nnot found in workspace".into()));
        }
    }

    let class_id = Uuid::now_v7();
    let row = sqlx::query!(
        r#"
        INSERT INTO classes (
            id, workspace_id, channel_id, tutor_user_id, title, description,
            location, starts_at, ends_at, recurrence_rule,
            self_checkin_enabled, created_by_user_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
        RETURNING id, workspace_id, channel_id, tutor_user_id, title, description,
                  location, starts_at, ends_at, recurrence_rule, status,
                  self_checkin_enabled, created_at
        "#,
        class_id,
        ctx.workspace_id,
        req.channel_id,
        tutor_user_id,
        req.title,
        req.description,
        req.location,
        req.starts_at,
        req.ends_at,
        req.recurrence_rule,
        req.self_checkin_enabled,
        ctx.user_id
    )
    .fetch_one(&state.db)
    .await?;

    let tutor = sqlx::query!(
        r#"
        SELECT full_name FROM users WHERE id = $1
        "#,
        tutor_user_id
    )
    .fetch_one(&state.db)
    .await?;

    let is_online = row.location.starts_with("http://") || row.location.starts_with("https://");

    Ok((StatusCode::CREATED, Json(ClassResponse {
        id: row.id,
        workspace_id: row.workspace_id,
        channel_id: row.channel_id,
        tutor_user_id: row.tutor_user_id,
        tutor_full_name: tutor.full_name,
        title: row.title,
        description: row.description,
        location: row.location,
        is_online,
        starts_at: row.starts_at,
        ends_at: row.ends_at,
        recurrence_rule: row.recurrence_rule,
        status: row.status,
        self_checkin_enabled: row.self_checkin_enabled,
        created_at: row.created_at,
    })))
}


#[derive(Debug, Deserialize)]
pub struct ListClassesQuery {
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub tutor_id: Option<Uuid>,
}

pub async fn list_classes(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Query(q): Query<ListClassesQuery>,
) -> AppResult<Json<Vec<ClassResponse>>> {
    let from = q.from.unwrap_or_else(|| Utc::now() - chrono::Duration::days(30));
    let to = q.from.unwrap_or_else(|| Utc::now() + chrono::Duration::days(90));

    let rows = sqlx::query!(
        r#"
        SELECT c.id, c.workspace_id, c.channel_id, c.tutor_user_id, c.title,
               c.description, c.location, c.starts_at, c.ends_at,
               c.recurrence_rule, c.status, c.self_checkin_enabled, c.created_at,
               u.full_name as tutor_full_name
        FROM classes c
        JOIN users u ON u.id = c.tutor_user_id
        WHERE c.workspace_id = $1
          AND c.starts_at >= $2
          AND c.starts_at <= $3
          AND ($4::UUID IS NULL OR c.tutor_user_id = $4)
        ORDER BY c.starts_at ASC
        "#,
        ctx.workspace_id,
        from,
        to,
        q.tutor_id
    )
    .fetch_all(&state.db)
    .await?;

    let classes = rows.into_iter().map(|r| {
        let is_online = r.location.starts_with("http://") || r.location.starts_with("https://");
        ClassResponse {
            id: r.id,
            workspace_id: r.workspace_id,
            channel_id: r.channel_id,
            tutor_user_id: r.tutor_user_id,
            tutor_full_name: r.tutor_full_name,
            title: r.title,
            description: r.description,
            location: r.location,
            is_online,
            starts_at: r.starts_at,
            ends_at: r.ends_at,
            recurrence_rule: r.recurrence_rule,
            status: r.status,
            self_checkin_enabled: r.self_checkin_enabled,
            created_at: r.created_at,
        }
    }).collect();

    Ok(Json(classes))
}

pub async fn get_class(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(class_id): Path<Uuid>,
) -> AppResult<Json<ClassResponse>> {
    let row = sqlx::query!(
        r#"
        SELECT c.id, c.workspace_id, c.channel_id, c.tutor_user_id, c.title,
               c.description, c.location, c.starts_at, c.ends_at,
               c.recurrence_rule, c.status, c.self_checkin_enabled, c.created_at,
               u.full_name as tutor_full_name,
               wm.role as "role?"
        FROM classes c
        JOIN users u ON u.id = c.tutor_user_id
        LEFT JOIN workspace_members wm
            ON wm.workspace_id = c.workspace_id AND wm.user_id = $2
        WHERE c.id = $1
        "#,
        class_id,
        auth.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("class not found".into()))?;

    row.role.ok_or_else(|| AppError::NotFound("class not found".into()))?;

    let is_online = row.location.starts_with("http://") || row.location.starts_with("https://");

    Ok(Json(ClassResponse { 
        id: row.id, 
        workspace_id: row.workspace_id, 
        channel_id: row.channel_id, 
        tutor_user_id: row.tutor_user_id, 
        tutor_full_name: row.tutor_full_name, 
        title: row.title, 
        description: row.description, 
        location: row.location, 
        is_online, 
        starts_at: row.starts_at, 
        ends_at: row.ends_at, 
        recurrence_rule: row.recurrence_rule, 
        status: row.status, 
        self_checkin_enabled: row.self_checkin_enabled, 
        created_at: row.created_at 
    }))
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateClassRequest {
    #[validate(length(min = 2, max = 200))]
    pub title: Option<String>,

    pub description: Option<String>,

    #[validate(length(min = 1, max = 500))]
    pub location: Option<String>,

    pub starts_at: Option<DateTime<Utc>>,
    pub ends_at: Option<DateTime<Utc>>,

    pub self_checkin_enabled: Option<bool>,
}

pub async fn update_class(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(class_id): Path<Uuid>,
    Json(req): Json<UpdateClassRequest>,
) -> AppResult<StatusCode> {
    req.validate()?;

    let class = sqlx::query!(
        r#"
        SELECT c.workspace_id, c.tutor_user_id, c.starts_at, c.ends_at,
               wm.role as "role?"
        FROM classes c
        LEFT JOIN workspace_members wm
            ON wm.workspace_id = c.workspace_id AND wm.user_id = $2
        WHERE c.id = $1
        "#,
        class_id,
        &auth.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("class not found".into()))?;

    let role = class.role.ok_or_else(|| AppError::NotFound("class not found".into()))?;

    let is_admin = matches!(role.as_str(), "proprietor" | "admin");
    let is_tutor = class.tutor_user_id == auth.user_id;

    if !is_admin && !is_tutor {
        return Err(AppError::Forbidden("only assigned tutor or admin can edit".into()));
    }

    // Validate time consistency
    let new_starts = req.starts_at.unwrap_or(class.starts_at);
    let new_ends = req.ends_at.unwrap_or(class.starts_at);
    if new_ends <= new_starts {
        return Err(AppError::BadRequest("start time must be before end time".into()));
    }

    sqlx::query!(
        r#"
        UPDATE classes SET
            title = COALESCE($1, title),
            description = COALESCE($2, description),
            location = COALESCE($3, location),
            starts_at = COALESCE($4, starts_at),
            ends_at = COALESCE($5, ends_at),
            self_checkin_enabled = COALESCE($6, self_checkin_enabled)
        WHERE id = $7
        "#,
        req.title,
        req.description,
        req.location,
        req.starts_at,
        req.ends_at,
        req.self_checkin_enabled,
        class_id
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn cancel_class(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(class_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    let class = sqlx::query!(
        r#"
        SELECT c.tutor_user_id, wm.role as "role?"
        FROM classes c
        LEFT JOIN workspace_members wm
            ON wm.workspace_id = c.workspace_id AND wm.user_id = $2
        WHERE c.id = $1
        "#,
        class_id,
        auth.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("class not found".into()))?;

    let role = class.role.ok_or_else(|| AppError::NotFound("class not found".into()))?;

    let is_admin = matches!(role.as_str(), "proprietor" | "admin");
    let is_tutor = class.tutor_user_id == auth.user_id;

    if !is_admin && !is_tutor {
        return Err(AppError::Forbidden("only assigned tutor or admin can cancel".into()));
    }

    sqlx::query!(
        "UPDATE classes SET status = 'cancelled' WHERE id = $1",
        class_id
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}
