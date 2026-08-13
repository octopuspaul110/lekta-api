use aws_sdk_s3::config::auth;
use axum::{Json, extract::{Path, Query, State}};
use chrono::{DateTime, Utc};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{auth::extractor::AuthUser, error::{AppError, AppResult}, state::AppState};

#[derive(Debug, Deserialize, Validate)]
pub struct RegisterDeviceRequest {
    #[validate(length(min = 10, max = 512))]
    pub token: String,

    pub platform: String,
    pub device_name: Option<String>,
    pub app_version: Option<String>,
}

pub async fn register_device(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<RegisterDeviceRequest>
) -> AppResult<StatusCode> {
    req.validate()?;
    
    if matches!(req.platform.as_str(), "ios" | "android") {
        return Err(AppError::BadRequest("invalid platform".into()));
    }

    sqlx::query!(
        r#"
        INSERT INTO device_tokens (
            user_id, fcm_token, platform, device_name, app_version, is_active
        )
        VALUES ($1, $2, $3, $4, $5, TRUE)
        ON CONFLICT (fcm_token) DO UPDATE
        SET user_id = EXCLUDED.user_id,
            platform = EXCLUDED.platform,
            device_name = EXCLUDED.device_name,
            app_version = EXCLUDED.app_version,
            is_active = TRUE,
            last_seen_at = NOW()
        "#,
        auth.user_id,
        req.token,
        req.platform,
        req.device_name,
        req.app_version
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NOT_FOUND)
}

pub async fn unregister_device(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(token): Path<String>,
) -> AppResult<StatusCode> {
    sqlx::query!(
        "UPDATE device_tokens SET is_active = FALSE WHERE user_id = $1 AND fcm_token = $2",
        auth.user_id,
        token
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NOT_FOUND)
}

#[derive(Debug, Deserialize)]
pub struct ListNotificationQuery {
    #[serde(default = "default_limit")]
    pub limit: u32,
    
    pub unread_only: Option<bool>,
}
fn default_limit() -> u32 {50}

#[derive(Debug, Serialize)]
pub struct NotificationResponse {
    pub id: Uuid,
    pub notification_type: String,
    pub title: String,
    pub body: String,
    pub metadata: serde_json::Value,
    pub is_read: bool,
    pub created_at: DateTime<Utc>,
}


pub async fn list_notifications(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<ListNotificationQuery>,
) -> AppResult<Json<Vec<NotificationResponse>>> {
    let limit = q.limit.min(100) as i64;
    let unread_only = q.unread_only.unwrap_or(false);

    let rows = sqlx::query!(
        r#"
        SELECT id, type as "notification_type", title , body, metadata, is_read, created_at
        FROM notifications
        WHERE user_id = $1
          AND ($2 = FALSE OR is_read = FALSE)
        ORDER BY created_at DESC
        LIMIT $3
        "#,
        auth.user_id,
        unread_only,
        limit
    )
    .fetch_all(&state.db)
    .await?;

    let notifications = rows.into_iter().map(|r|                NotificationResponse{
            id: r.id,
            notification_type: r.notification_type,
            title: r.title,
            body: r.body,
            metadata: r.metadata,
            is_read: r.is_read,
            created_at: r.created_at,
        }).collect();

    Ok(Json(notifications))
}

pub async fn mark_read(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(notification_id): Path<Uuid>
) -> AppResult<StatusCode> {
    let result = sqlx::query!(
        "UPDATE notifications SET is_read = TRUE WHERE id = $1 AND user_id = $2 AND is_read = FALSE",
        notification_id,
        auth.user_id
    ).execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("notification not found".into()));
    }

    Ok(StatusCode::NO_CONTENT)
}

