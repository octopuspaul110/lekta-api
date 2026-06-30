use axum::{Json, extract::{Path, State}};
use chrono::{DateTime, Utc};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{auth::extractor::AuthUser, channels::types::{ChannelType, PostPermission, Visibility}, error::{AppError, AppResult}, state::AppState, workspaces::{extractor::WorkspaceContext}};

static CHANNEL_NAME_REGEX: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"^[a-z0-9][a-z0-9-]{0,30}[a-z0-9]$").unwrap()
});

#[derive(Debug, Serialize)]
pub struct ChannelResponse {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub display_name: String,
    pub description: Option<String>,
    pub channel_type: ChannelType,
    pub visibility: Visibility,
    pub post_permission: PostPermission,
    pub is_default: bool,
    pub archived: bool,
    pub message_count: i32,
    pub last_message_at: Option<DateTime<Utc>>,
    pub unread_count: Option<i64>,
    pub created_at: DateTime<Utc>,
}


pub async fn list_channels(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
) -> AppResult<Json<Vec<ChannelResponse>>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            c.id, c.workspace_id, c.name, c.display_name, c.description,
            c.channel_type, c.visibility, c.post_permission, c.is_default,
            c.archived, c.message_count, c.last_message_at, c.created_at,
            (
                SELECT COUNT(*) FROM messages m
                WHERE m.channel_id = c.id
                  AND m.deleted = FALSE
                  AND m.thread_parent_id IS NULL
                  AND m.created_at > COALESCE(cm.last_read_at, '1970-01-01'::timestamptz)
            ) as unread_count
        FROM channels c
        LEFT JOIN channel_members cm ON cm.channel_id = c.id AND cm.user_id = $2
        WHERE c.workspace_id = $1
          AND c.archived = FALSE
          AND (
            c.visibility = 'public'
            OR cm.user_id IS NOT NULL
          )
        ORDER BY c.last_message_at DESC NULLS LAST, c.created_at DESC
        "#,
        ctx.workspace_id,
        ctx.user_id
    )
    .fetch_all(&state.db)
    .await?;

    let channels: Result<Vec<_>, AppError> = rows
        .into_iter()
        .map(|r| {
            let channel_type: ChannelType = 
            serde_json::from_value(serde_json::json!(r.channel_type))
                .map_err(|_| AppError::Internal("invalid channel_type".into()))?;
            
            let visibility: Visibility = serde_json::from_value(serde_json::json!(r.visibility))
                .map_err(|_| AppError::Internal("invalid visibility".into()))?;

            let post_permission: PostPermission = serde_json::from_value(serde_json::json!(r.post_permission))
                .map_err(|_| AppError::Internal("invalid post_permission".into()))?;

            Ok(ChannelResponse {
                id: r.id,
                workspace_id: r.workspace_id,
                name: r.name,
                display_name: r.display_name,
                description: r.description,
                channel_type,
                visibility,
                post_permission,
                is_default: r.is_default,
                archived: r.archived,
                message_count: r.message_count,
                last_message_at: r.last_message_at,
                unread_count: r.unread_count,
                created_at: r.created_at,
            })
        })
        .collect();

    Ok(Json(channels?))
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateChannelResponse {
    #[validate(regex(path = "CHANNEL_NAME_REGEX"))]
    pub name: String,

    #[validate(length(min = 2, max = 50))]
    pub display_name: String,

    pub description: Option<String>,

    pub channel_type: ChannelType,

    #[serde(default = "default_visibility")]
    pub visibility: Visibility,

    #[serde(default = "default_post_permission")]
    pub post_permission: PostPermission,
}

fn default_visibility() -> Visibility { Visibility::Public}
fn default_post_permission() -> PostPermission { PostPermission::Everyone}

pub async fn create_channel(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Json(req): Json<CreateChannelResponse>
) -> AppResult<(StatusCode,Json<ChannelResponse>)>{
    req.validate()?;

    if !ctx.role.is_admin_or_above() {
        return Err(AppError::Forbidden("tutor role required".into()));
    }

    // announcement channels, only admins can create announcement channel
    if matches!(req.channel_type, ChannelType::Announcement) && !ctx.role.is_admin_or_above() {
        return Err(AppError::Forbidden("admin role required for annoucement channel".into()));
    }

    // channel must be unique
    let existing = sqlx::query!(
        "SELECT id FROM channels WHERE workspace_id = $1 AND name = $2",
        ctx.workspace_id,
        req.name
    )
    .fetch_optional(&state.db)
    .await?;

    if existing.is_some() {
        return Err(AppError::Conflict("channel name already exists".into()));
    }

    let channel_id = Uuid::now_v7();
    let channel_type_str = serde_json::to_value(&req.channel_type)?.as_str().unwrap().to_string();
    let visibility_str = serde_json::to_value(&req.visibility)?.as_str().unwrap().to_string();
    let post_permission_str = serde_json::to_value(&req.post_permission)?.as_str().unwrap().to_string();

    let mut tx = state.db.begin().await?;

    let row = sqlx::query!(
        r#"
        INSERT INTO channels (
            id, workspace_id, name, display_name, description,
            channel_type, visibility, post_permission,
            created_by_user_id, is_default
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, FALSE)
        RETURNING id, workspace_id, name, display_name, description,
                  channel_type, visibility, post_permission,
                  is_default, archived, message_count, last_message_at, created_at
        "#,
        channel_id,
        ctx.workspace_id,
        req.name,
        req.display_name,
        req.description,
        channel_type_str,
        visibility_str,
        post_permission_str,
        ctx.user_id
    )
    .fetch_one(&mut *tx)
    .await?;

    if matches!(req.visibility, Visibility::Private) {
        sqlx::query!(
            "INSERT INTO channel_members (channel_id, user_id) VALUES ($1, $2)",
            channel_id,
            ctx.user_id
        )
        .execute(&mut * tx)
        .await?;
    }

    tx.commit().await?;

    let channel_type: ChannelType = serde_json::from_value(serde_json::json!(row.channel_type))?;
    let visibility: Visibility = serde_json::from_value(serde_json::json!(row.visibility))?;
    let post_permission: PostPermission = serde_json::from_value(serde_json::json!(row.post_permission))?;

    Ok((
        StatusCode::CREATED,
        Json(ChannelResponse {
            id: row.id,
            workspace_id: row.workspace_id,
            name: row.name,
            display_name: row.display_name,
            description: row.description,
            channel_type,
            visibility,
            post_permission,
            is_default: row.is_default,
            archived: row.archived,
            message_count: row.message_count,
            last_message_at: row.last_message_at,
            unread_count: None,
            created_at: row.created_at,
        })
    ))
}

pub async fn get_channel(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>
) -> AppResult<Json<ChannelResponse>> {
    let row = sqlx::query!(
        r#"
        SELECT
            c.id, c.workspace_id, c.name, c.display_name, c.description,
            c.channel_type, c.visibility, c.post_permission, c.is_default,
            c.archived, c.message_count, c.last_message_at, c.created_at
        FROM channels c
        WHERE c.id = $1
        "#,
        channel_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("channel not found".into()))?;

    // Verify user access:
    // - for public channel, user has to be a workspace member
    // - for private channel, user has to be a channel member
    let is_member = sqlx::query!(
        r#"
        SELECT 1 as "exists!"
        FROM workspace_members wm
        WHERE wm.workspace_id = $1 AND wm.user_id = $2 AND wm.status = 'active'
        "#,
        row.workspace_id,
        auth.user_id,
    )
    .fetch_optional(&state.db)
    .await?
    .is_some();

    if !is_member {
        return Err(AppError::NotFound("channel not found".into()));
    }

    if row.visibility == "private" {
        let has_channel_membership = sqlx::query!(
            r#"
            SELECT 1 AS "exists!"
            FROM channel_members
            WHERE channel_id = $1 AND user_id = $2
            "#,
            channel_id,
            auth.user_id
        )
        .fetch_optional(&state.db)
        .await?
        .is_some();

        if has_channel_membership {
            return Err(AppError::NotFound("channel not found".into()));
        }
    }

    let channel_type: ChannelType = serde_json::from_value(serde_json::json!(row.channel_type))?;
    let visibility: Visibility = serde_json::from_value(serde_json::json!(row.visibility))?;
    let post_permission: PostPermission = serde_json::from_value(serde_json::json!(row.post_permission))?;

    Ok(Json( ChannelResponse { 
        id: row.id, 
        workspace_id: row.workspace_id, 
        name: row.name, 
        display_name: row.display_name, 
        description: row.description, 
        channel_type, 
        visibility, 
        post_permission, 
        is_default: row.is_default, 
        archived: row.archived, 
        message_count: row.message_count, 
        last_message_at: row.last_message_at, 
        unread_count: None, 
        created_at: row.created_at,
    }))
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateChannelRequest {
    #[validate(length(min = 2, max = 50))]
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub post_permission: Option<PostPermission>,
}

pub async fn update_channel(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
    Json(req): Json<UpdateChannelRequest>,
) -> AppResult<StatusCode> {
    req.validate()?;

    // fetch channel to be updated and check that the user is the creator or workspace admin
    let row = sqlx::query!(
        r#"
        SELECT c.workspace_id, c.created_by_user_id, wm.role as "role?"
        FROM channels c
        LEFT JOIN workspace_members wm
            ON wm.workspace_id = c.workspace_id AND wm.user_id = $2
        WHERE c.id = $1
        "#,
        channel_id,
        auth.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("channel not found".into()))?;

    let role = row.role.ok_or_else(|| AppError::NotFound("channel not found".into()))?;
    let is_creator = row.created_by_user_id == auth.user_id;
    let is_admin = role == "proprietor" || role == "admin";

    if !is_creator && !is_admin {
        return Err(AppError::Forbidden("creator or admin only".into()));
    }

    let post_permission_str = req.post_permission.as_ref().map(|p| {
        serde_json::to_value(p).unwrap().as_str().unwrap().to_string()
    });

    sqlx::query!(
        r#"
        UPDATE channels SET
            display_name = COALESCE($1, display_name),
            description = COALESCE($2, description),
            post_permission = COALESCE($3, post_permission)
        WHERE id = $4
        "#,
        req.display_name,
        req.description,
        post_permission_str,
        channel_id
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn archive_channel(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>
) -> AppResult<StatusCode> {
    let row = sqlx::query!(
        r#"
        SELECT c.workspace_id, c.created_by_user_id, c.name, wm.role as "role?"
        FROM channels c
        LEFT JOIN workspace_members wm
            ON wm.workspace_id = c.workspace_id AND wm.user_id = $2
        WHERE c.id = $1 
        "#,
        channel_id,
        auth.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("channel not found".into()))?;

    let role = row.role.ok_or_else(|| AppError::NotFound("channel not found".into()))?;
    let is_creator = row.created_by_user_id == auth.user_id;
    let is_admin = role == "proprietor" || role == "admin";

    if !is_creator && !is_admin {
        return Err(AppError::Forbidden("creator or admin only".into()));
    }

    // prevent archiving default channel
    if row.name == "general" {
        return Err(AppError::BadRequest("cannot archive default channel #general".into()));
    }

    sqlx::query!(
        "UPDATE channels SET archived = true, updated_at = now() WHERE id = $1",
        channel_id
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}