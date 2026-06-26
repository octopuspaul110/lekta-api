use axum::{Json, extract::State};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use validator::Validate;
use uuid::Uuid;

use crate::{auth::extractor::AuthUser, error::{AppResult,AppError}, state::AppState, workspaces::types::{PaymentMode, WorkspaceRole}};

#[derive(Debug, Deserialize, Validate)]
pub struct CreateWorkspaceRequest {
    #[validate(length(min = 2, max = 100))]
    pub name : String,

    #[validate(regex(
        path = "SLUG_REGEX",
        message = "slug must be lowercase letters, digits, hyphens; 3-50 chars"
    ))]
    pub slug: String,
    pub description: Option<String>,

    #[validate(length(min = 1))]
    pub focus_areas: Vec<String>,

    #[serde(default)]
    pub payment_mode: Option<PaymentMode>
    
}

// regex compiled at startup
static SLUG_REGEX : std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(||{
    regex::Regex::new(r"^[a-z0-9][a-z0-9-]{1,48}[a-z0-9]$").unwrap()
});

#[derive(Debug, Serialize)]
pub struct WorkspaceResponse {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub focus_areas: Vec<String>,
    pub payment_mode: PaymentMode,
    pub subscription_status: String,
    pub role: WorkspaceRole, 
}

pub async fn create_workspace(

    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateWorkspaceRequest>
) -> AppResult<(StatusCode, Json<WorkspaceResponse>)> {
    req.validate()?;

    if !auth.email_verified {
        return Err(AppError::Forbidden("email must be verified".into()));
    }

    let existing = sqlx::query!(
        "SELECT id FROM workspaces WHERE slug = $1 AND deleted_at IS NULL",
        req.slug
    )
    .fetch_optional(&state.db)
    .await?;

    if existing.is_some() {
        return Err(AppError::Conflict("slug already taken".into()));
    }

    let workspace_id = Uuid::now_v7();
    let payment_mode = req.payment_mode.unwrap_or(PaymentMode::LektaManaged);
    let subaccount_status = match payment_mode {
        PaymentMode::External => "not_applicable",
        _ => "pending"
    };

    // lock database
    let mut tx = state.db.begin().await?;

    let payment_mode_str = serde_json::to_value(&payment_mode)?
    .as_str()
    .ok_or_else(|| AppError::Internal("payment_mode serialization failed".into()))?
    .to_string();

    // insert workspace into table
    let workspace = sqlx::query!(
        "
        INSERT INTO workspaces (
            id, name, slug, description, focus_areas,
            proprietor_user_id, payment_mode, paystack_subaccount_status,
            trial_ends_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW() + INTERVAL '14 days')
        RETURNING id, name, slug, description, focus_areas, payment_mode, subscription_status
        ",
        workspace_id,
        req.name,
        req.slug,
        req.description,
        &req.focus_areas,
        auth.user_id,
        payment_mode_str,
        subaccount_status
    )
    .fetch_one(&mut *tx)
    .await?;

    // insert proprietor as workspace member
    sqlx::query!(
        r#"
        INSERT INTO workspace_members (workspace_id, user_id, role, status)
        VALUES ($1, $2, 'proprietor', 'active')
        "#,
        workspace_id,
        auth.user_id
    )
    .execute(&mut *tx)
    .await?;

    // insert default general channel into workspace
    sqlx::query!(
        r#"
        INSERT INTO channels(
            workspace_id, name, display_name,channel_type,
            visibility, post_permission, created_by_user_id, is_default
        )
        VALUES ($1, 'general', 'General', 'general', 'public', 'everyone', $2, TRUE)
        "#,
        workspace_id,
        auth.user_id
    )
    .execute(&mut *tx)
    .await?;

    // insert default announcement channel
    sqlx::query!(
        r#"
            INSERT INTO channels(
                workspace_id, name, display_name, channel_type, visibility, post_permission, created_by_user_id, is_default
            )
            VALUES ($1, 'announcement', 'Announcement', 'announcement', 'public', 'admins_only', $2, TRUE)
        "#,
        workspace_id,
        auth.user_id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;


    Ok((
        StatusCode::CREATED,
        Json(WorkspaceResponse { 
            id: workspace.id, 
            name: workspace.name, 
            slug: workspace.slug, 
            description: workspace.description, 
            focus_areas: workspace.focus_areas, 
            payment_mode, 
            subscription_status: workspace.subscription_status, 
            role: WorkspaceRole::Proprietor }),
    ))
}

pub async fn list_my_workspaces(
    State(state): State<AppState>,
    auth: AuthUser
) -> AppResult<Json<Vec<WorkspaceResponse>>> {
    let rows = sqlx::query!(
        "
            SELECT w.id, w.name, w.slug, w.description, w.focus_areas,
               w.payment_mode, w.subscription_status, wm.role
            FROM workspaces w
            JOIN workspace_members wm ON wm.workspace_id = w.id
            WHERE wm.user_id = $1
            AND wm.status = 'active'
            AND w.deleted_at IS NULL
            ORDER BY w.created_at DESC
        ",
        auth.user_id
    )
    .fetch_all(&state.db)
    .await?;

    let workspaces : Result<Vec<_>,AppError> = rows
        .into_iter()
        .map(|r|{
            let payment_mode: PaymentMode = serde_json::from_value(serde_json::json!(r.payment_mode))
                .map_err(|_| AppError::Internal("invalid payment_mode".into()))?;
            let role: WorkspaceRole = serde_json::from_value(serde_json::json!(r.role))
                .map_err(|_| AppError::Internal("invalid role".into()))?;

            Ok(WorkspaceResponse {
                id: r.id,
                name: r.name,
                slug: r.slug,
                description: r.description,
                focus_areas: r.focus_areas,
                payment_mode,
                subscription_status: r.subscription_status,
                role
            })
        })
        .collect();

    Ok(Json(workspaces?))
}