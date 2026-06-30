use axum::{Json, extract::{Path, State}};
use hyper::StatusCode;
use serde::Deserialize;
use uuid::Uuid;

use crate::{auth::extractor::AuthUser, error::{AppError, AppResult}, state::AppState};

#[derive(Debug, Deserialize)]
pub struct AddMemberRequest {
    pub user_id: Option<Uuid>
}

pub async fn add_or_join_channel(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
    Json(req): Json<AddMemberRequest>
) -> AppResult<StatusCode> {
    let channel = sqlx::query!(
        r#"
        SELECT c.workspace_id, c.visibility, c.created_by_user_id, c.archived, wm.role as "role?"
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

    let requester_role = channel.role.ok_or_else(|| AppError::NotFound("channel not found".into()))?;
    
    if channel.archived {
        return Err(AppError::BadRequest("channel is archived".into()));
    }

    let target_user_id = req.user_id.unwrap_or(auth.user_id);

    // Self-join logic
    if target_user_id == auth.user_id {
        if channel.visibility == "private" {
            return Err(AppError::Forbidden("private channels require an invite".into()));
        }
        // public channel and self = Ok, fall through to UPSERT
    } else {
        // adding someone else will require admin or channel creator
        let is_admin = requester_role == "proprietor" || requester_role == "admin";
        let is_creator = channel.created_by_user_id == auth.user_id;

        if !is_admin && !is_creator {
            return Err(AppError::Forbidden("admin or creator only".into()));
        }

        // verify target is a workspace member
        let target_in_workspace = sqlx::query!(
            r#"
            SELECT 1 as "exists!"
            FROM workspace_members
            WHERE workspace_id = $1 AND user_id = $2 AND status = 'active'
            "#,
            channel.workspace_id,
            target_user_id
        )
        .fetch_optional(&state.db)
        .await?;

        if target_in_workspace.is_none() {
            return Err(AppError::NotFound("user is not a workspace member".into()));
        }
    }

    sqlx::query!(
        r#"
        INSERT INTO channel_members (channel_id, user_id)
        VALUES ($1, $2)
        ON CONFLICT (channel_id, user_id) DO NOTHING
        "#,
        channel_id,
        target_user_id
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_channel_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((channel_id, target_user_id)): Path<(Uuid,Uuid)>,
) -> AppResult<StatusCode> {
    let channel = sqlx::query!(
        r#"
        SELECT
            c.workspace_id, c.name, c.visibility, c.created_by_user_id, c.is_default,
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

    let requester_role = channel.role
    .ok_or_else(|| AppError::NotFound("channel not found".into()))?;

    // self-leave logic
    if target_user_id == auth.user_id {
        // cant leave default channels
        if channel.is_default {
            return Err(AppError::BadRequest("cannot leave defalt channels".into()));
        }
    } else {
        // removing someone else will require admin or channel creator
        let is_admin = requester_role == "proprietor" || requester_role == "admin";
        let is_creator = channel.created_by_user_id == auth.user_id;

        if !is_admin && !is_creator {
            return Err(AppError::Forbidden("admin or creator only".into()))
        }
    }

    sqlx::query!(
        "DELETE FROM channel_members WHERE channel_id = $1 AND user_id = $2",
        channel_id,
        target_user_id
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub struct MarkRequest {
    pub last_read_message_id: Uuid,
}
pub async fn mark_channel_read(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(channel_id): Path<Uuid>,
    Json(req): Json<MarkRequest>
) -> AppResult<StatusCode> {
    // Verify the channel exists and user has access
    let channel = sqlx::query!(
        r#"
        SELECT
            c.workspace_id, c.visibility,
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

    let _role = channel.role.ok_or_else(|| AppError::NotFound("channel not found".into()))?;

    // for private channels, verify channel_member row exists
    if channel.visibility == "private" {
        let is_member = sqlx::query!(
            r#"
            SELECT 1 as "exists!"
            FROM channel_members
            WHERE channel_id = $1 AND user_id = $2
            "#,
            channel_id,
            auth.user_id
        )
        .fetch_optional(&state.db)
        .await?
        .is_some();
        
        if !is_member {
            return Err(AppError::NotFound("channel not found".into()));
        }

        //  Verify message belongs to channel
        let msg = sqlx::query!(
            "SELECT 1 as \"exists!\" FROM messages WHERE id = $1 AND channel_id = $2",
            req.last_read_message_id,
            channel_id
        )
        .fetch_optional(&state.db)
        .await?;
        
        if msg.is_none() {
            return Err(AppError::BadRequest("message not in this channel".into()));
        }
    }
    // UPSERT — insert row if missing, update last_read fields
    sqlx::query!(
        r#"
        INSERT INTO channel_members (channel_id, user_id, last_read_message_id, last_read_at)
        VALUES ($1, $2, $3, NOW())
        ON CONFLICT (channel_id, user_id)
        DO UPDATE SET
            last_read_message_id = EXCLUDED.last_read_message_id,
            last_read_at = NOW()
        "#,
        channel_id,
        auth.user_id,
        req.last_read_message_id
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}