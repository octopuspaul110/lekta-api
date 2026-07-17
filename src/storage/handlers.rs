use std::time::Duration;

use axum::{Json, extract::{Query, State}};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{auth::extractor::AuthUser, error::{AppError, AppResult}, state::{AppState}};

#[derive(Debug, Deserialize, Validate)]
pub struct RequestUploadRequest {
    /// One of: avatar, tutor_photo, workspace_avatar, message, assignment, submission, exam_media, note
    pub purpose: String,

    #[validate(length(min = 2, max = 100))]
    pub content_type: String,

    #[validate(range(min = 1, max = 52428800))] // validat 50 mb
    pub size_bytes: i64,

    /// Contextual ID: workspace_id for workspace-scoped, or other IDs
    pub workspace_id: Option<Uuid>,
    pub message_id: Option<Uuid>,
    pub assignment_id: Option<Uuid>,
    pub submission_id: Option<Uuid>,
    pub question_id: Option<Uuid>,
    pub note_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct RequestUploadResponse {
    pub upload_url: String,
    pub s3_key: String,
    pub expires_in_seconds: u64,
}

pub async fn request_upload(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<RequestUploadRequest>,
) -> AppResult<Json<RequestUploadResponse>> {
    req.validate()?;

    // Basic content type validation
    if !matches!(req.content_type.as_str(), "image/jpeg" | "image/png" | "image/webp" | "image/gif" |
        "application/pdf" |
        "audio/mpeg" | "audio/mp4" | "audio/webm" | "audio/ogg" |
        "video/mp4" | "video/webm"
    ) {
            return Err(AppError::BadRequest("content type not llowed".into()));
    }
    let extension = match req.content_type.as_str() {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "application/pdf" => "pdf",
        "audio/mpeg" => "mp3",
        "audio/mp4" => "m4a",
        "audio/webm" => "webm",
        "audio/ogg" => "ogg",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        _ => "bin",
    };

    let file_id = Uuid::now_v7();

    let key = match req.purpose.as_str() {
        "avatar" => format!("avatars/{}/{}.{}",auth.user_id, file_id, extension),
        "tutor_photo" => {
            let ws = req.workspace_id.ok_or_else(|| AppError::BadRequest("workspace_id required".into()))?;
            format!("tutor-photos/{}/{}/{}.{}",ws ,auth.user_id , file_id, extension)
        }
        "workspace_avatar" => {
            let ws = req.workspace_id.ok_or_else(|| AppError::BadRequest("workspace_id required".into()))?;
            format!("workspace-avatar/{}/{}.{}", ws, file_id, extension)
        }
        "message" => {
            let ws = req.workspace_id.ok_or_else(|| AppError::BadRequest("workspace_id required".into()))?;
            let msg= req.message_id.ok_or_else(|| AppError::BadRequest("message_id required".into()))?;
            format!("messages/{}/{}/{}.{}", ws, msg, file_id, extension)
        }
        "assignment" => {
            let ws = req.workspace_id.ok_or_else(|| AppError::BadRequest("workspace_id required".into()))?;
            let a = req.assignment_id.ok_or_else(|| AppError::BadRequest("assignment_id required".into()))?;
            format!("assignments/{}/{}/{}.{}", ws, a, file_id, extension)
        }
        "submission" => {
            let ws = req.workspace_id.ok_or_else(|| AppError::BadRequest("workspace_id required".into()))?;
            let s = req.submission_id.ok_or_else(|| AppError::BadRequest("submission_id required".into()))?;
            format!("submissions/{}/{}/{}.{}", ws, s, file_id, extension)
        }
        "exam_media" => {
            let ws = req.workspace_id.ok_or_else(|| AppError::BadRequest("workspace_id required".into()))?;
            let q = req.question_id.ok_or_else(|| AppError::BadRequest("question_id required".into()))?;
            format!("exam-media/{}/{}/{}.{}", ws, q, file_id, extension)
        }
        "note" => {
            let ws = req.workspace_id.ok_or_else(|| AppError::BadRequest("workspace_id required".into()))?;
            let n = req.note_id.ok_or_else(|| AppError::BadRequest("note_id required".into()))?;
            format!("notes/{}/{}/{}.{}", ws, n, file_id, extension)
        }
        _ => return Err(AppError::BadRequest("invalid purpose".into())),
    };

    let url = state.s3
        .presign_upload(&key, &req.content_type, Duration::from_secs(900))
        .await?;

    Ok(Json(RequestUploadResponse { 
        upload_url: url, 
        s3_key: key, 
        expires_in_seconds: 900 
    }))
}

#[derive(Debug, Deserialize)]
pub struct DownloadUrlQuery {
    pub key: String,
}

#[derive(Debug, Serialize)]
pub struct DownloadUrlResponse {
    pub download_url: String,
    pub expires_in_seconds: u64,
}

pub async fn get_download_url(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<DownloadUrlQuery>,
) -> AppResult<Json<DownloadUrlResponse>> {
    if q.key.starts_with("avatars/") {
        // all authenticated users can download avatars
    } else if q.key.starts_with("tutor-photos/") || q.key.starts_with("workspace-avatars/") {
        let parts: Vec<&str> = q.key.split('/').collect();
        if parts.len() < 2 {
            return Err(AppError::BadRequest("invalid key".into()));
        }
        let ws_id = Uuid::parse_str(parts[1]).map_err(|_| AppError::BadRequest("invalid workspace id in key".into()))?;

        let is_member = sqlx::query!(
            r#"
            SELECT 1 as "exists!" FROM workspace_members
            WHERE workspace_id = $1 AND user_id = $2 AND status = 'active'
            "#,
            ws_id,
            auth.user_id
        )
        .fetch_optional(&state.db)
        .await?;

        if is_member.is_none() {
            return Err(AppError::Forbidden("not a workspace member".into()));
        }
    } else if q.key.starts_with("messages/") ||
              q.key.starts_with("assignments/") ||
              q.key.starts_with("submissions/") ||
              q.key.starts_with("exam-media/") ||
              q.key.starts_with("notes/") {
        let parts: Vec<&str> = q.key.split('/').collect();
        if parts.len() < 2 {
            return Err(AppError::BadRequest("invalid workspace id in key".into()))?;
        }
        let ws_id = Uuid::parse_str(parts[1]).map_err(|_| AppError::BadRequest("invalid workspace id in key".into()))?;

        let is_member = sqlx::query!(
            r#"
            SELECT 1 as "exists!" FROM workspace_members
            WHERE workspace_id = $1 AND user_id = $2 AND status = 'active'
            "#,
            ws_id,
            auth.user_id
        )
        .fetch_optional(&state.db)
        .await?;

        if is_member.is_none() {
            return Err(AppError::Forbidden("not a workspace member".into()));
        }
    } else {
        return Err(AppError::BadRequest("unknown key prefix".into()));
    }

    let url = state.s3.presign_download(&q.key, Duration::from_secs(3600)).await?;

    Ok(Json(DownloadUrlResponse { 
        download_url: url, 
        expires_in_seconds: 3600, 
    }))

}
