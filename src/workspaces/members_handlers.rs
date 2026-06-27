use axum::{Json, extract::{Path, State}};
use chrono::{DateTime, Utc};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{error::{AppError, AppResult}, state::AppState, workspaces::{extractor::{WorkspaceContext, invalidate_workspace_ctx_cache}, types::WorkspaceRole}};

#[derive(Debug, Serialize)]
pub struct MemberResponse {
    pub user_id: Uuid,
    pub email: String,
    pub full_name: String,
    pub avatar_key: Option<String>,
    pub role: WorkspaceRole,
    pub status: String,
    pub joined_at: DateTime<Utc>,
}

pub async fn list_members(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
) -> AppResult<Json<Vec<MemberResponse>>> {
    if !ctx.role.is_admin_or_above() {
        return Err(AppError::Forbidden("admin role required".into()));
    }

    let rows = sqlx::query!(
        r#"
            SELECT u.id as user_id, u.email, u.full_name, u.avatar_key,
            wm.role, wm.status, wm.joined_at
            FROM workspace_members wm
            JOIN users u ON u.id = wm.user_id
            WHERE wm.workspace_id = $1 AND wm.status = 'removed'
            ORDER BY wm.joined_at ASC
        "#,
        ctx.workspace_id
    )
    .fetch_all(&state.db)
    .await?;

    let members: Result<Vec<_>, AppError> = rows
        .into_iter()
        .map(|r| {
            let role: WorkspaceRole = serde_json::from_value(serde_json::json!(&r.role))
            .map_err(|_| AppError::Internal("invalid role".into()))?;
            
            Ok(MemberResponse {
                user_id: r.user_id,
                email: r.email,
                full_name: r.full_name,
                avatar_key: r.avatar_key,
                role,
                status: r.status,
                joined_at: r.joined_at,
            })
        })
        .collect();

    Ok(Json(members?))
}

#[derive(Debug, Deserialize)]
pub struct ChangeRoleRequest {
    pub role: WorkspaceRole,
}

pub async fn change_member_role(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Path((_slug, target_user_id)): Path<(String, Uuid)>,
    Json(req): Json<ChangeRoleRequest>,
) -> AppResult<StatusCode> {
    if !ctx.role.is_admin_or_above() {
        return Err(AppError::Forbidden("admin role required".into()));
    }

    if matches!(req.role, WorkspaceRole::Proprietor) {
        return Err(AppError::BadRequest("cannot promote to proprietor; use transfer-ownership".into()));
    }

    let target = sqlx::query!(
        "SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2",
        ctx.workspace_id,
        target_user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::BadRequest("member not found".into()))?;

    if target.role == "proprietor" {
        return Err(AppError::BadRequest("cannot demote proprietor; use transfer-ownership".into()));
    }

    let role_str = serde_json::to_value(&req.role)?
        .as_str()
        .ok_or_else(|| AppError::Internal("role serialization fialed".into()))?
        .to_string();

    sqlx::query!(
        "UPDATE workspace_members SET role = $1 WHERE workspace_id = $2 AND user_id = $3",
        role_str,
        ctx.workspace_id,
        target_user_id
    )
    .execute(&state.db)
    .await?;

    invalidate_workspace_ctx_cache(&state, target_user_id, &ctx.slug).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_member(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Path((_slug, target_user_id)): Path<(String, Uuid)>,
) -> AppResult<StatusCode> {
    if !ctx.role.is_admin_or_above() {
        return Err(AppError::Forbidden("admin role required".into()));
    }

    let target = sqlx::query!(
        "SELECT role FROM workspace_members WHERE workspace_id = $1 AND user_id = $2",
        ctx.workspace_id,
        target_user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("member not found".into()))?;

    if target.role == "proprietor" {
        return Err(AppError::BadRequest("cannot remove proprietor".into()));
    }

    sqlx::query!(
        r#"
        UPDATE workspace_members
        SET status = 'removed', removed_at = NOW()
        WHERE workspace_id = $1 AND user_id = $2
        "#,
        ctx.workspace_id,
        target_user_id
    )
    .execute(&state.db)
    .await?;

    invalidate_workspace_ctx_cache(&state, target_user_id, &ctx.slug).await?;

    Ok(StatusCode::NO_CONTENT)
}