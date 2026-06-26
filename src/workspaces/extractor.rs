use std::collections::HashMap;

use axum::extract::{FromRequestParts, Path};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use axum::http::request::Parts;

use crate::{auth::extractor::AuthUser, error::{AppError, AppResult}, state::AppState, workspaces::types::{PaymentMode, WorkspaceRole}};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceContext {
    pub workspace_id: Uuid,
    pub slug: String,
    pub role: WorkspaceRole,
    pub payment_mode: PaymentMode,
    pub user_id: Uuid,
}

impl FromRequestParts<AppState> for WorkspaceContext {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        
        // Get the authenticated user first
        let auth = AuthUser::from_request_parts(parts, state).await?;

        // Extract slug from path
        let Path(path_params): Path<HashMap<String, String>> = 
            Path::from_request_parts(parts,state)
            .await
            .map_err(|_| AppError::Internal("missing slug in route".into()))?;

        let slug = path_params
            .get("slug")
            .ok_or_else(|| AppError::Internal("slug param missing".into()))?
            .clone();

        // try cache first
        let cache_key = format!("workspace_ctx:{}:{}",auth.user_id,slug);
        let mut redis = state.redis.clone();

        if let Ok(cached) = redis.get::<_, String>(&cache_key).await {
            if let Ok(ctx) = serde_json::from_str::<WorkspaceContext>(&cached) {
                return Ok(ctx);
            }
        }

        // cached miss goes to postgress
        let row = sqlx::query!(
            "
                SELECT w.id, w.payment_mode, wm.role
                FROM workspaces w
                JOIN workspace_members wm ON wm.workspace_id = w.id
                WHERE w.slug = $1
                    AND w.deleted_at IS NULL
                    AND wm.user_id = $2
                    AND wm.status = 'active'
            ",
            slug,
            auth.user_id
        )
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("Workspace not found".into()))?;

        let role: WorkspaceRole = serde_json::from_value(serde_json::json!(row.role))
        .map_err(|_| AppError::Internal("invalid role in db".into()))?;

        let payment_mode : PaymentMode = serde_json::from_value(serde_json::json!(row.payment_mode))
        .map_err(|_| AppError::Internal("invalid payment_mode in db".into()))?;

        let ctx = WorkspaceContext {
            workspace_id: row.id,
            slug: slug.clone(),
            role,
            payment_mode,
            user_id: auth.user_id
        };

        // cache for 60 seconds
        if let Ok(json) = serde_json::to_string(&ctx) {
            let _ = redis
                .set_ex::<_,_,()>(&cache_key, json, 60)
                .await;
        }

        Ok(ctx)
    }
}

/// Helper for invalidating cache when membership changes
pub async fn invalidate_workspace_ctx_cache(
    state: &AppState,
    user_id: Uuid,
    slug: &str
) -> AppResult<()> {
    let mut redis = state.redis.clone();
    let key = format!("workspace_ctx:{}:{}",user_id,slug);
    let _: Result<(),_> =  redis.del(&key).await;
    
    Ok(())
}