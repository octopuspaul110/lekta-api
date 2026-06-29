use axum::{Json, extract::State};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use validator::Validate;
use uuid::Uuid;

use crate::{auth::extractor::AuthUser, error::{AppError, AppResult}, state::AppState, workspaces::{extractor::{WorkspaceContext, invalidate_workspace_ctx_cache}, types::{OnboardingStep, PaymentMode, WorkspaceRole}}};

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
    pub payment_mode: Option<PaymentMode>,

    // Optional custom onboarding sequence, If omitted, defaults are applied.
    pub onboarding_steps: Option<Vec<OnboardingStep>>
    
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

    let onboarding_steps = req
        .onboarding_steps
        .unwrap_or_else(|| OnboardingStep::default_sequence(&req.name));

    let onboarding_json = serde_json::to_value(&onboarding_steps)?;

    // insert workspace into table
    let workspace = sqlx::query!(
        "
        INSERT INTO workspaces (
            id, name, slug, description, focus_areas,
            proprietor_user_id, payment_mode, paystack_subaccount_status,
            onboarding_steps,trial_ends_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW() + INTERVAL '14 days')
        RETURNING id, name, slug, description, focus_areas, payment_mode, subscription_status
        ",
        workspace_id,
        req.name,
        req.slug,
        req.description,
        &req.focus_areas,
        auth.user_id,
        payment_mode_str,
        subaccount_status,
        onboarding_json
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

pub async fn get_workspace(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
) -> AppResult<Json<WorkspaceResponse>> {
    let row = sqlx::query!(
        r#"
            SELECT id, name, slug, description, focus_areas, payment_mode,subscription_status,
                   avatar_key,cover_image_key, student_count, tutor_count, trial_ends_at,  created_at
            FROM workspaces
            WHERE id = $1 AND deleted_at IS NULL
        "#,
        &ctx.workspace_id
    )
    .fetch_one(&state.db)
    .await?;

    let payment_mode: PaymentMode = serde_json::from_value(serde_json::json!(row.payment_mode))
        .map_err(|_| AppError::Internal("invalid payment_mode".into()))?;

    Ok(
        Json(
            WorkspaceResponse {
                id: row.id,
                name: row.name,
                slug: row.slug,
                description: row.description,
                focus_areas: row.focus_areas,
                payment_mode,
                subscription_status: row.subscription_status,
                role: ctx.role,
            })
    )
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateWorkspaceRequest {
    #[validate(length(min = 2, max = 100))]
    pub name: Option<String>,

    pub description: Option<String>,

    #[validate(regex(path = "SLUG_REGEX"))]
    pub slug: Option<String>,

    pub focus_areas: Option<Vec<String>>,

    pub settings: Option<serde_json::Value>,
}

pub async fn update_workspace(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Json(req): Json<UpdateWorkspaceRequest>,
) -> AppResult<Json<WorkspaceResponse>> {
    req.validate()?;

    if !ctx.role.is_admin_or_above() {
        return Err(AppError::Forbidden("admin role required".into()));
    }

    let mut tx = state.db.begin().await?;

    // if slug is changing, check uniqueness and record redirect
    if let Some(new_slug) = &req.slug {
        if new_slug != &ctx.slug {
            let existing = sqlx::query!(
                "SELECT id FROM workspaces WHERE slug = $1 AND deleted_at IS NULL",
                new_slug
            )
            .fetch_optional(&state.db)
            .await?;
            
            if existing.is_some() {
                return Err(AppError::Conflict("slug already taken".into()));
            }
            sqlx::query!(
                "
                INSERT INTO workspace_slug_redirects (old_slug, workspace_id) VALUES ($1, $2)
                ",
                ctx.slug,
                ctx.workspace_id
            )
            .execute(&state.db)
            .await?;
        }
    }

    let row = sqlx::query!(
        r#"
            UPDATE workspaces
            SET 
                name = COALESCE($1, name),
                description = COALESCE($2, description),
                slug = COALESCE($3, slug),
                focus_areas = COALESCE($4, focus_areas),
                settings = COALESCE($5, settings)
            WHERE id = $6
            RETURNING id, name, slug, description, focus_areas, payment_mode, subscription_status
        "#,
        req.name,
        req.description,
        req.slug,
        req.focus_areas.as_deref(),
        req.settings,
        ctx.workspace_id
    )
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    
    // invalidate cache for the requester
    invalidate_workspace_ctx_cache(&state, ctx.user_id, &ctx.slug).await?;

    let payment_mode: PaymentMode = serde_json::from_value(serde_json::json!(row.payment_mode))
        .map_err(|_| AppError::Internal("invalid payment_mode".into()))?;

    Ok(Json(
        WorkspaceResponse { 
            id: row.id, 
            name: row.name, 
            slug: row.slug, 
            description: row.description, 
            focus_areas: row.focus_areas, 
            payment_mode, 
            subscription_status: row.subscription_status, 
            role: ctx.role
        }))
}

#[derive(Debug, Deserialize)]
pub struct TransferOwnershipRequest {
    pub new_proprietor_user_id: Uuid,
}

pub async fn transfer_ownership(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Json(req): Json<TransferOwnershipRequest>,
) -> AppResult<StatusCode> {

    if !matches!(ctx.role, WorkspaceRole::Proprietor) {
        return Err(AppError::Forbidden("proprietor only".into()));
    }

    if req.new_proprietor_user_id == ctx.user_id {
        return Err(AppError::BadRequest("cannot transfer to self".into()));
    }

    // taget being transferred to has to be an admin in the workspace
    let target = sqlx::query!(
        r#"
        SELECT role FROM workspace_members
        WHERE workspace_id = $1 AND user_id = $2 AND status = 'active'
        "#,
        ctx.workspace_id,
        req.new_proprietor_user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("user is not a member".into()))?;

    if target.role != "admin" {
        return Err(AppError::BadRequest("target must first be an admin to become a proprietor".into()));
    }

    let mut tx = state.db.begin().await?;

    sqlx::query!(
        r#"
        UPDATE workspace_members 
        set role = 'admin'
        WHERE workspace_id = $1 AND user_id = $2
        "#,
        ctx.workspace_id,
        ctx.user_id
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!(
        r#"
        UPDATE workspace_members
        SET role = 'proprietor'
        WHERE workspace_id = $1 AND user_id = $2
        "#,
        ctx.workspace_id,
        req.new_proprietor_user_id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    // invalidate cache for both users
    invalidate_workspace_ctx_cache(&state, ctx.user_id, &ctx.slug).await?;
    invalidate_workspace_ctx_cache(&state, req.new_proprietor_user_id, &ctx.slug).await?;
    
    tracing::info!(
        workspace_id = %ctx.workspace_id,
        from_user = %ctx.user_id,
        to_user = %req.new_proprietor_user_id,
        "ownership transferred"
    );

    Ok(StatusCode::NO_CONTENT)
}


pub async fn delete_workspace(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
) -> AppResult<StatusCode> {
    if !matches!(ctx.role, WorkspaceRole::Proprietor) {
        return Err(AppError::Forbidden("proprietor only".into()));
    }

    sqlx::query!(
        "UPDATE workspaces SET deleted_at = NOW() where id = $1 AND deleted_at IS NULL",
        ctx.workspace_id
    )
    .execute(&state.db)
    .await?;

    invalidate_workspace_ctx_cache(&state, ctx.user_id, &ctx.slug).await?;

    tracing::info!(
        workspace_id = %&ctx.workspace_id,
        deleted_by = %&ctx.user_id,
        "workspace soft-deleted"
    );

    Ok(StatusCode::NO_CONTENT)
}


#[derive(Debug, Deserialize)]
pub struct UpdateOnboardingRequest {
    pub onboarding_steps: Vec<OnboardingStep>
}

#[derive(Debug,Serialize)]
pub struct OnboardingResponse {
    pub onboarding_steps: Vec<OnboardingStep>
}

pub async fn update_onboarding(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Json(req): Json<UpdateOnboardingRequest>
) -> AppResult<Json<OnboardingResponse>> {
    if !ctx.role.is_admin_or_above() {
        return Err(AppError::Forbidden("admin role required".into()));
    }

    let onboarding_json = serde_json::to_value(&req.onboarding_steps)?;

    sqlx::query!(
        "UPDATE workspaces SET onboarding_steps = $1 WHERE id = $2",
        onboarding_json,
        ctx.workspace_id
    )
    .execute(&state.db)
    .await?;

    Ok(Json(OnboardingResponse { onboarding_steps: req.onboarding_steps }))
}

pub async fn get_onboarding(
    State(state): State<AppState>,
    ctx: WorkspaceContext
) -> AppResult<Json<OnboardingResponse>> {
    let row = sqlx::query!(
        "SELECT onboarding_steps FROM workspaces WHERE ID = $1",
        ctx.workspace_id
    )
    .fetch_one(&state.db)
    .await?;

    let onboarding_steps: Vec<OnboardingStep> = serde_json::from_value(row.onboarding_steps)
    .map_err(|_| AppError::Internal("invalid onboarding_steps Json".into()))?;

    Ok(Json(OnboardingResponse { onboarding_steps }))
}