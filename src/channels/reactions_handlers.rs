use axum::{Json, extract::{Path, State}};
use hyper::StatusCode;
use serde::Deserialize;
use uuid::Uuid;

use crate::{auth::extractor::AuthUser, error::{AppError, AppResult}, state::AppState};

#[derive(Debug, Deserialize)]
pub struct AddReactionRequest {
    pub emoji: String
}

pub async fn add_reaction(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(message_id): Path<Uuid>,
    Json(req): Json<AddReactionRequest>
) -> AppResult<StatusCode>{

    let message = sqlx::query!(
        r#"
        SELECT m.workspace_id, m.channel_id, m.deleted,
            c.visibility as "visibility?",
            wm.role as "role?",
            (SELECT 1 FROM channel_members
                WHERE channel_id = m.channel_id AND user_id = $2) as "is_channel_member?"
        FROM messages m
        LEFT JOIN channels c ON c.id = m.channel_id
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
        return Err(AppError::NotFound("message not found".into()));
    }

    message.role.ok_or_else(|| AppError::NotFound("message not found".into()))?;

    if message.visibility.as_deref() == Some("private") && message.is_channel_member.is_none() {
        return Err(AppError::NotFound("message not found".into()));
    }

    sqlx::query!(
        r#"
        INSERT INTO message_reactions (message_id, user_id, emoji)
        VALUES ($1, $2, $3)
        ON CONFLICT DO NOTHING
        "#,
        message_id,
        auth.user_id,
        req.emoji
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_reaction(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((message_id,emoji)): Path<(Uuid, String)>
) -> AppResult<StatusCode>{
    
    sqlx::query!(
        r#"
        DELETE FROM message_reactions
        WHERE message_id = $1 AND user_id = $2 AND emoji = $3
        "#,
        message_id,
        auth.user_id,
        emoji
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}