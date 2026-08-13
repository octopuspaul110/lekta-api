use uuid::Uuid;

use crate::{error::{AppError, AppResult}, notifications, state::AppState};

pub async fn notify_users(
    state: &AppState,
    user_id: Uuid,
    workspace_id: Option<Uuid>,
    title: &str,
    body: &str,
    data: Option<serde_json::Value>,
    notification_type: &str
) -> AppResult<()>{
    let devices = sqlx::query!(
        "SELECT fcm_token FROM device_tokens WHERE user_id = $1 AND is_active = TRUE",
        user_id
    )
    .fetch_all(&state.db)
    .await?;

    let notification_id = Uuid::now_v7();

    sqlx::query!(
        r#"
        INSERT INTO notifications (
            id, user_id, workspace_id, type,
            title, body, metadata
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        notification_id,
        user_id,
        workspace_id,
        notification_type,
        title,
        body,
        data.clone().unwrap_or(serde_json::json!({}))
    )
    .execute(&state.db)
    .await?;

    for device in devices {
        match state.fcm.send(&device.fcm_token, title, body, data.clone()).await {
            Ok(true) => {},
            Ok(false) => {
                // mark invalid token inactive
                sqlx::query!(
                    "UPDATE device_tokens SET is_active = FALSE WHERE fcm_token = $1",
                    device.fcm_token
                )
                .execute(&state.db)
                .await
                .ok();
            }
            Err(e) => {
                tracing::warn!(error = ?e, "failed to send push notification");
            },
        }
    }

    Ok(())
}