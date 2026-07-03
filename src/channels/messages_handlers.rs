
use axum::{Json, extract::{Path, Query, State}};
use chrono::{DateTime, Duration, Utc};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{auth::extractor::AuthUser, error::{AppError, AppResult}, state::AppState};

#[derive(Debug, Serialize)]
pub struct MessageResponse {
    pub id: Uuid,
    pub channel_id: Option<Uuid>,
    pub workspace_id: Uuid,
    pub sender_user_id: Uuid,
    pub sender_full_name: String,
    pub sender_avatar_key: Option<String>,
    pub content: String,
    pub attachments: serde_json::Value,
    pub thread_parent_id: Option<Uuid>,
    pub thread_reply_count: i32,
    pub edited: bool,
    pub edited_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct ListMessageQuery {
    pub before: Option<Uuid>,
    pub after: Option<Uuid>,
    #[serde(default = "default_limit")]
    pub limit: u32
}
fn default_limit() -> u32 {50}

pub async fn list_messages(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
    Query(q): Query<ListMessageQuery>,
) -> AppResult<Json<Vec<MessageResponse>>> {
    let limit = q.limit.min(100) as i64;

    let channel = sqlx::query!(
        r#"
        SELECT c.workspace_id, c.visibility, wm.role as "role?"
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
    .ok_or_else(|| AppError::NotFound("channel not available".into()))?;

    channel.role.ok_or_else(||AppError::NotFound("channel not found".into()))?;

    if channel.visibility == "private" {
        let is_cm = sqlx::query!(
            "SELECT 1 AS \"exists!\" FROM channel_members WHERE channel_id = $1 AND user_id = $2",
            channel_id,
            auth.user_id
        )
        .fetch_optional(&state.db)
        .await?;

        if is_cm.is_none() {
            return Err(AppError::NotFound("channel not found".into()))
        }
    }

    // Resolve cursor timestamp if provided
    let cursor_ts: Option<DateTime<Utc>> = if let Some(cursor_id) = q.before.or(q.after) {
        sqlx::query!(
            "SELECT created_at FROM messages WHERE id = $1",
            cursor_id
        )
        .fetch_optional(&state.db)
        .await?
        .map(|r|r.created_at)
    } else {
        None
    };

    // Two querypaths: forward (after) vs backward (before / initial)
    let messages: Vec<MessageResponse> = if let Some(ts) = cursor_ts {
        if q.after.is_some() {
            sqlx::query!(
                r#"
                SELECT m.id, m.channel_id, m.workspace_id, m.sender_user_id,
                       m.content, m.attachments, m.thread_parent_id, m.thread_reply_count,
                       m.edited, m.edited_at, m.created_at,
                       u.full_name as sender_full_name, u.avatar_key as sender_avatar_key
                FROM messages m
                JOIN users u ON u.id = m.sender_user_id
                WHERE m.channel_id = $1
                  AND m.deleted = FALSE
                  AND m.thread_parent_id IS NULL
                  AND m.created_at > $2
                ORDER BY m.created_at ASC
                LIMIT $3
                "#,
                channel_id,
                ts,
                limit
            )
            .fetch_all(&state.db)
            .await?
            .into_iter()
            .map(|r| MessageResponse {
                id: r.id,
                channel_id: r.channel_id,
                workspace_id: r.workspace_id,
                sender_user_id: r.sender_user_id,
                sender_full_name: r.sender_full_name,
                sender_avatar_key: r.sender_avatar_key,
                content: r.content,
                attachments: r.attachments,
                thread_parent_id: r.thread_parent_id,
                thread_reply_count: r.thread_reply_count,
                edited: r.edited,
                edited_at: r.edited_at,
                created_at: r.created_at,
            }).collect()
        } else {
            sqlx::query!(
                r#"
                SELECT m.id, m.channel_id, m.workspace_id, m.sender_user_id,
                       m.content, m.attachments, m.thread_parent_id, m.thread_reply_count,
                       m.edited, m.edited_at, m.created_at,
                       u.full_name as sender_full_name, u.avatar_key as sender_avatar_key
                FROM messages m
                JOIN users u ON u.id = m.sender_user_id
                WHERE m.channel_id = $1
                  AND m.deleted = FALSE
                  AND m.thread_parent_id IS NULL
                  AND m.created_at < $2
                ORDER BY m.created_at DESC
                LIMIT $3
                "#,
                channel_id,
                ts,
                limit
            )
            .fetch_all(&state.db)
            .await?
            .into_iter()
            .map(|r| MessageResponse {
                id: r.id,
                channel_id: r.channel_id,
                workspace_id: r.workspace_id,
                sender_user_id: r.sender_user_id,
                sender_full_name: r.sender_full_name,
                sender_avatar_key: r.sender_avatar_key,
                content: r.content,
                attachments: r.attachments,
                thread_parent_id: r.thread_parent_id,
                thread_reply_count: r.thread_reply_count,
                edited: r.edited,
                edited_at: r.edited_at,
                created_at: r.created_at,
            })
            .collect()
        }
    } else {
        // initial load, most recent messages
        sqlx::query!(
            r#"
            SELECT m.id, m.channel_id, m.workspace_id, m.sender_user_id,
                   m.content, m.attachments, m.thread_parent_id, m.thread_reply_count,
                   m.edited, m.edited_at, m.created_at,
                   u.full_name as sender_full_name, u.avatar_key as sender_avatar_key
            FROM messages m
            JOIN users u ON u.id = m.sender_user_id
            WHERE m.channel_id = $1
              AND m.deleted = FALSE
              AND m.thread_parent_id IS NULL
            ORDER BY m.created_at DESC
            LIMIT $2
            "#,
            channel_id,
            limit
        )
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(|r| MessageResponse {
            id: r.id,
            channel_id: r.channel_id,
            workspace_id: r.workspace_id,
            sender_user_id: r.sender_user_id,
            sender_full_name: r.sender_full_name,
            sender_avatar_key: r.sender_avatar_key,
            content: r.content,
            attachments: r.attachments,
            thread_parent_id: r.thread_parent_id,
            thread_reply_count: r.thread_reply_count,
            edited: r.edited,
            edited_at: r.edited_at,
            created_at: r.created_at,

        })
        .collect()
    };

    Ok(Json(messages))
}

#[derive(Debug, Deserialize, Validate)]
pub struct SendMessageRequest {
    #[validate(length(max = 4000))]
    pub content: String,

    #[serde(default)]
    pub attachments: serde_json::Value,
    pub thread_parent_id: Option<Uuid>,
}

pub async fn send_message(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
    Json(req): Json<SendMessageRequest>,
) -> AppResult<(StatusCode,Json<MessageResponse>)> {
    req.validate()?;

    // empty messages disallowed unless they have attachments
    let has_attachments = req.attachments.as_array().map(|a| !a.is_empty()).unwrap_or(false);
    if req.content.trim().is_empty() && !has_attachments {
        return Err(AppError::BadRequest("message must have content or attachments".into()));
    }

    // Fetch channel and check access and check post permission
    let channel = sqlx::query!(
        r#"
        SELECT c.workspace_id, c.visibility, c.post_permission, c.archived,
               wm.role as "role?"
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

    let role = channel.role.ok_or_else(|| AppError::NotFound("channel not found".into()))?;

    if channel.archived {
        return Err(AppError::BadRequest("channel is archived".into()))
    }

    if channel.visibility == "private" {
        let is_cm = sqlx::query!(
            "SELECT 1 as \"exists!\" FROM channel_members WHERE channel_id = $1 AND user_id = $2",
            channel_id,
            auth.user_id
        )
        .fetch_optional(&state.db)
        .await?;

        if is_cm.is_none() {
            return Err(AppError::NotFound("channel not found".into()));
        }
    }
    // Enforce post_permission
    let can_post = match channel.post_permission.as_str() {
        "everyone" => true,
        "tutos_and_admins" => matches!(role.as_str(), "proprietor" | "admin" | "tutor"),
        "admins_only" => matches!(role.as_str(), "proprietor" | "admin"),
        _ => false,
    };

    if !can_post {
        return Err(AppError::Forbidden("you do not have permission to post here".into()));
    }

    // if thread reply, Verify parent exists in this channel and is not a thread reply
    if let Some(parent_id) = req.thread_parent_id {
        let parent = sqlx::query!(
            r#"
            SELECT thread_parent_id FROM messages
            WHERE id = $1 and channel_id = $2 AND deleted = FALSE
            "#,
            parent_id,
            channel_id
        )
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::BadRequest("thread parent not found".into()))?;

        if parent.thread_parent_id.is_some() {
            return Err(AppError::BadRequest("cannot reply to a thread reply".into()));
        }
    }

    let message_id = Uuid::now_v7();

    let row = sqlx::query!(
        r#"
        INSERT INTO messages (
            id, channel_id, workspace_id, sender_user_id,
            content, attachments, thread_parent_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id, channel_id, workspace_id, sender_user_id,
                  content, attachments, thread_parent_id, thread_reply_count,
                  edited, edited_at, created_at
        "#,
        message_id,
        channel_id,
        channel.workspace_id,
        auth.user_id,
        req.content,
        req.attachments,
        req.thread_parent_id
    )
    .fetch_one(&state.db)
    .await?;

    tracing::info!(
        message_id = %message_id,
        channel_id = %channel_id,
        sender_user_id = %auth.user_id,
        has_thread_parent = req.thread_parent_id.is_some(),
        "message sent"
    );

    // Get sender details
    let sender = sqlx::query!(
        "SELECT full_name, avatar_key FROM users WHERE id = $1",
        auth.user_id
    )
    .fetch_one(&state.db)
    .await?;

    // update channel.last_message_at (only for top-level messages)
    if req.thread_parent_id.is_none() {
        sqlx::query!(
            r#"
            UPDATE channels SET last_message_at = NOW(), message_count = message_count + 1 WHERE id = $1
            "#,
            channel_id
        )
        .execute(&state.db)
        .await?;
    } else {
        // Thread reply - increment parents reply count
        sqlx::query!(
            r#"
            UPDATE messages SET thread_reply_count = thread_reply_count + 1 WHERE id = $1
            "#,
            req.thread_parent_id.unwrap()
        )
        .execute(&state.db)
        .await?;
    }

    // TODO: parse mentions, enqueue notifications when jobs queue is built
    if req.content.contains('@') {
        tracing::info!(
            message_id = %message_id,
            "message contains @mentions (TODO: parse and notify)"
        );
    }

    Ok((StatusCode::CREATED, Json(MessageResponse { 
        id: row.id, 
        channel_id: row.channel_id, 
        workspace_id: row.workspace_id, 
        sender_user_id: row.sender_user_id, 
        sender_full_name: sender.full_name, 
        sender_avatar_key: sender.avatar_key, 
        content: row.content, 
        attachments: row.attachments, 
        thread_parent_id: row.thread_parent_id, 
        thread_reply_count: row.thread_reply_count, 
        edited: row.edited, 
        edited_at: row.edited_at, 
        created_at: row.created_at, 
    })))
}

#[derive(Debug, Deserialize, Validate)]
pub struct EditMessageRequest {
    #[validate(length(min = 1, max = 4000))]
    pub content: String,
}

pub async fn edit_message(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(message_id): Path<Uuid>,
    Json(req): Json<EditMessageRequest>,
) -> AppResult<StatusCode> {
    req.validate()?;

    let message = sqlx::query!(
        r#"
        SELECT sender_user_id, created_at, deleted FROM messages WHERE id = $1
        "#,
        message_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("message not found".into()))?;

    if message.deleted {
        return Err(AppError::BadRequest("cannot edit deleted message".into()));
    }

    if message.sender_user_id != auth.user_id {
        return Err(AppError::Forbidden("only sender can edit".into()));
    }

    // 24-hour edit window
    if Utc::now() - message.created_at > Duration::hours(24) {
        return Err(AppError::BadRequest("edit window wxpired".into()));
    }
    sqlx::query!(
        r#"
        UPDATE messages
        SET content = $1, edited = TRUE, edited_at = NOW()
        WHERE id = $2
        "#,
        req.content,
        message_id
    )
    .execute(&state.db)
    .await?;

    tracing::info!(
        message_id = %message_id,
        edited_by = %auth.user_id,
        "message edited"
    );

    Ok(StatusCode::NO_CONTENT)
}
pub async fn delete_message(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(message_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    let message = sqlx::query!(
        r#"
        SELECT m.sender_user_id, m.workspace_id, m.deleted,
               m.channel_id, m.thread_parent_id,
               wm.role as "role?"
        FROM messages m
        LEFT JOIN workspace_members wm
            ON wm.workspace_id = m.workspace_id AND wm.user_id = $2
        WHERE m.id = $1
        "#,
        message_id,
        auth.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("message not found".into()))?;

    if message.deleted {
        return Err(AppError::BadRequest("already deleted".into()));
    }

    let role = message.role.ok_or_else(|| AppError::NotFound("message not found".into()))?;

    let is_sender = message.sender_user_id == auth.user_id;
    let is_admin = matches!(role.as_str(), "proprietor" | "admin");

    if !is_sender && !is_admin {
        return Err(AppError::Forbidden("only sender or admin can delete".into()));
    }

    sqlx::query!(
        r#"
        UPDATE messages SET deleted = TRUE, deleted_at = NOW(), deleted_by_user_id = $1 WHERE id = $2
        "#,
        auth.user_id,
        message_id
    )
    .execute(&state.db)
    .await?;

    // decrement counts
    if let Some(parent_id) = message.thread_parent_id {
        sqlx::query!(
            "UPDATE messages SET thread_reply_count = thread_reply_count - 1 WHERE id = $1",
            parent_id
        )
        .execute(&state.db)
        .await?;
    } else if let Some(channel_id) = message.channel_id {
        sqlx::query!(
            r#"
            UPDATE channels SET message_count = message_count - 1 WHERE id = $1
            "#,
            &channel_id
        )
        .execute(&state.db)
        .await?;
    }

    Ok(StatusCode::NO_CONTENT)

}

pub async fn get_thread(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(parent_id): Path<Uuid>
) -> AppResult<Json<Vec<MessageResponse>>> {
    // verify access via the parent's channel
    let parent = sqlx::query!(
        r#"
        SELECT m.workspace_id, m.channel_id, c.visibility as "visibility?", wm.role as "role?"
        FROM messages m
        LEFT JOIN channels c ON c.id = m.channel_id
        LEFT JOIN workspace_members wm
            ON wm.workspace_id = m.workspace_id AND wm.user_id = $2
        WHERE m.id = $1 AND m.deleted = FALSE
        "#,
        parent_id,
        auth.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("message not found".into()))?;

    parent.role.ok_or_else(|| AppError::NotFound("message not found".into()))?;

    if let Some(visibility) = parent.visibility {
        if visibility == "private" {
            let is_cm = sqlx::query!(
                "SELECT 1 as \"exists!\" FROM channel_members WHERE channel_id = $1 AND user_id = $2",
                parent.channel_id.unwrap(),
                auth.user_id
            )
            .fetch_optional(&state.db)
            .await?;

            if is_cm.is_none() {
                return Err(AppError::NotFound("message not found".into()));
            }
        }
    }

    let rows = sqlx::query!(
        r#"
        SELECT m.id, m.channel_id, m.workspace_id, m.sender_user_id,
               m.content, m.attachments, m.thread_parent_id, m.thread_reply_count,
               m.edited, m.edited_at, m.created_at,
               u.full_name as sender_full_name, u.avatar_key as sender_avatar_key
        FROM messages m
        JOIN users u ON u.id = m.sender_user_id
        WHERE m.thread_parent_id = $1
          AND m.deleted = FALSE
        ORDER BY m.created_at ASC
        "#,
        parent_id
    )
    .fetch_all(&state.db)
    .await?;

    let messages = rows.into_iter().map(|r| MessageResponse {
        id: r.id,
        channel_id: r.channel_id,
        workspace_id: r.workspace_id,
        sender_user_id: r.sender_user_id,
        sender_full_name: r.sender_full_name,
        sender_avatar_key: r.sender_avatar_key,
        content: r.content,
        attachments: r.attachments,
        thread_parent_id: r.thread_parent_id,
        thread_reply_count: r.thread_reply_count,
        edited: r.edited,
        edited_at: r.edited_at,
        created_at: r.created_at,
    }).collect();

    Ok(Json(messages))
}