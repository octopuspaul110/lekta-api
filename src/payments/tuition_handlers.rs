use axum::{Json, extract::{Path, State}};
use chrono::{DateTime, Utc};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{auth::extractor::AuthUser, error::{AppError, AppResult}, state::AppState, workspaces::extractor::WorkspaceContext};

#[derive(Debug, Serialize)]
pub struct TuitionPlanResponse {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub amount_kobo: i64,
    pub duration_days: i32,
    pub is_active: bool,
    pub enrollment_count: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateTuitionPlanRequest {
    #[validate(length(min = 2, max = 100))]
    pub name: String,

    pub description: Option<String>,

    #[validate(range(min = 0))]
    pub amount_kobo: i64,

    #[validate(range(min = 1, max = 3650))]
    pub duration_days: i32
}

pub async fn create_tuition_plan(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Json(req): Json<CreateTuitionPlanRequest>,
) -> AppResult<(StatusCode, Json<TuitionPlanResponse>)> {
    req.validate()?;

    if !ctx.role.is_admin_or_above() {
        return Err(AppError::Forbidden("admin role required".into()));
    }

    let plan_id = Uuid::now_v7();

    let row = sqlx::query!(
        r#"
        INSERT INTO tuition_plans (
            id, workspace_id, name, description, amount_kobo, duration_days,
            created_by_user_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id, workspace_id, name, description, amount_kobo,
                  duration_days, is_active, enrollment_count, created_at
        "#,
        plan_id,
        ctx.workspace_id,
        req.name,
        req.description,
        req.amount_kobo,
        req.duration_days,
        ctx.user_id
    )
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(TuitionPlanResponse { 
        id: row.id, 
        workspace_id: row.workspace_id, 
        name: row.name, 
        description: row.description, 
        amount_kobo: row.amount_kobo, 
        duration_days: row.duration_days, 
        is_active: row.is_active, 
        enrollment_count: row.enrollment_count, 
        created_at: row.created_at,   
    })))
}

pub async fn list_tuition_plans(
    State(state): State<AppState>,
    ctx: WorkspaceContext
) -> AppResult<Json<Vec<TuitionPlanResponse>>> {
    // Students see only active plans; admins see all
    let show_all = ctx.role.is_admin_or_above();

    let rows = sqlx::query!(
        r#"
        SELECT id, workspace_id, name, description, amount_kobo, duration_days,
               is_active, enrollment_count, created_at
        FROM tuition_plans
        WHERE workspace_id = $1
          AND deleted_at IS NULL
          AND ($2 OR is_active = TRUE)
        ORDER BY amount_kobo ASC, created_at ASC
        "#,
        ctx.workspace_id,
        show_all
    )
    .fetch_all(&state.db)
    .await?;

    let plans = rows.into_iter().map(|r| {
        TuitionPlanResponse {
            id: r.id,
            workspace_id: r.workspace_id,
            name: r.name,
            description: r.description,
            amount_kobo: r.amount_kobo,
            duration_days: r.duration_days,
            is_active: r.is_active,
            enrollment_count: r.enrollment_count,
            created_at: r.created_at,
        }
    }).collect();
    
    Ok(Json(plans))
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateTuitionPlanRequest {
    #[validate(length(min = 2, max = 100))]
    pub name: Option<String>,

    pub description: Option<String>,

    #[validate(range(min = 0))]
    pub amount_kobo: Option<i64>,

    #[validate(range(min = 1, max = 3650))]
    pub duration_days: Option<i32>,

    pub is_active: Option<bool>,
}

pub async fn update_tution_plan(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(plan_id): Path<Uuid>,
    Json(req): Json<UpdateTuitionPlanRequest>,
) -> AppResult<StatusCode> {
    req.validate()?;

    // fetch plan and check requester is workspace admin
    let plan = sqlx::query!(
        r#"
        SELECT tp.workspace_id, wm.role as "role?"
        FROM tuition_plans tp
        LEFT JOIN workspace_members wm
            ON wm.workspace_id = tp.workspace_id AND wm.user_id = $2
        WHERE tp.id = $1 AND tp.deleted_at IS NULL
        "#,
        plan_id,
        auth.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("tuition plan not found".into()))?;

    let role = plan.role.ok_or_else(|| AppError::NotFound("tuition plan not found".into()))?;

    if !matches!(role.as_str(), "proprietor" | "admin") {
        return Err(AppError::Forbidden("admin role required".into()));
    }

    sqlx::query!(
        r#"
        UPDATE tuition_plans
        SET name = COALESCE($1, name),
            description = COALESCE($2, description),
            amount_kobo = COALESCE($3, amount_kobo),
            duration_days = COALESCE($4, duration_days),
            is_active = COALESCE($5, is_active)
        WHERE id = $6
        "#,
        req.name,
        req.description,
        req.amount_kobo,
        req.duration_days,
        req.is_active,
        plan_id
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_tution_plan(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(plan_id): Path<Uuid>
) -> AppResult<StatusCode> {
    let plan = sqlx::query!(
        r#"
        SELECT tp.workspace_id, wm.role as "role?"
        FROM tuition_plans tp
        LEFT JOIN workspace_members wm
            ON wm.workspace_id = tp.workspace_id AND wm.user_id = $2
        WHERE tp.id = $1 AND tp.deleted_at IS NULL
        "#,
        plan_id,
        auth.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("tuition plan npt found".into()))?;

    let role = plan.role.ok_or_else(|| AppError::NotFound("tuition not found".into()))?;

    if !matches!(role.as_str(),  "proprietor" | "admin") {
        return Err(AppError::Forbidden("admin role required".into()));
    }
    sqlx::query!(
        r#"
        UPDATE tuition_plans SET deleted_at = NOW() WHERE id = $1
        "#,
        plan_id
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}
