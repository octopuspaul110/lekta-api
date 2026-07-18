use axum::{Json, extract::State, http::StatusCode};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{auth::extractor::AuthUser, error::{AppError, AppResult}, state::AppState, workspaces::{extractor::WorkspaceContext, types::PaymentMode}};

#[derive(Debug, Serialize)]
pub struct InitiateEnrollementResponse {
    pub enrollment_id: Uuid,
    pub payment_reference: String,
    pub authorization_url: String,
    pub amount_kobo: i64,
}

#[derive(Debug, Deserialize)]
pub struct InitiateEnrollementRequest {
    pub tuition_plan_id: Uuid
}

pub async fn initiate_enrollment(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    auth: AuthUser,
    Json(req): Json<InitiateEnrollementRequest>
) -> AppResult<(StatusCode, Json<InitiateEnrollementResponse>)> {

    // Workspace must accept Paystack payments
    if matches!(ctx.payment_mode, PaymentMode::External) {
        return Err(AppError::BadRequest("this workspace uses external payments; contact the center directly".into()));
    }

    // Fetch the plan + validate
    let plan = sqlx::query!(
        r#"
        SELECT id, amount_kobo, duration_days, is_active
        FROM tuition_plans
        WHERE id = $1 AND workspace_id = $2 AND deleted_at IS NULL
        "#,
        req.tuition_plan_id,
        ctx.workspace_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("tuition plan not found".into()))?;

    if !plan.is_active {
        return Err(AppError::BadRequest("plan is not active".into()));
    }

    // Fetch workspace details for Paystack call
    let workspace = sqlx::query!(
        r#"
        SELECT paystack_subaccount_code, paystack_subaccount_status,
               platform_fee_basis_points
        FROM workspaces WHERE id = $1
        "#,
        ctx.workspace_id
    )
    .fetch_one(&state.db)
    .await?;

    let subaccount_code = workspace.paystack_subaccount_code.ok_or_else(|| AppError::BadRequest("workspace has not completed payment onboarding".into()))?;

    if workspace.paystack_subaccount_status != "active" {
        return Err(AppError::BadRequest("workspace payment onboarding is not active".into()))?;
    }

    // Compute platform fee
    let platform_fee_kobo = plan.amount_kobo * workspace.platform_fee_basis_points as i64 / 10000;

    // Generate a reference
    let reference = format!("lekta_enr_{}", nanoid::nanoid!(10));

    let enrollment_id = Uuid::now_v7();
    let payment_id = Uuid::now_v7();

    let mut tx = state.db.begin().await?;

    // Create pending enrollment
    sqlx::query!(
        r#"
        INSERT INTO enrollments (
            id, workspace_id, student_user_id, tuition_plan_id,
            status, enrollment_source
        )
        VALUES ($1, $2, $3, $4, 'pending', 'paystack')
        "#,
        enrollment_id,
        ctx.workspace_id,
        auth.user_id,
        plan.id
    )
    .execute(&mut *tx)
    .await?;

    // Create pending payment
    sqlx::query!(
        r#"
        INSERT INTO payments (
            id, reference, workspace_id, payer_user_id, enrollment_id,
            payment_purpose, amount_kobo, platform_fee_kobo, status
        )
        VALUES ($1, $2, $3, $4, $5, 'tuition', $6, $7, 'pending')
        "#,
        payment_id,
        reference,
        ctx.workspace_id,
        auth.user_id,
        enrollment_id,
        plan.amount_kobo,
        platform_fee_kobo
    )
    .execute(&mut *tx)
    .await?;

    // Link payment to enrollment
    sqlx::query!(
        "UPDATE enrollments SET payment_id = $1 WHERE id = $2",
        payment_id,
        enrollment_id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    // Call Paystack
    let callback_url = format!("{}/api/v1/payments/callback", state.config.app_base_url);
    let metadata = serde_json::json!({
        "enrollment_id": enrollment_id.to_string(),
        "workspace_id": ctx.workspace_id.to_string(),
    });

    let init = state.paystack.initialize_transaction(
        &auth.email, 
        plan.amount_kobo, 
        &reference, 
        Some(&subaccount_code), 
        &callback_url, 
        metadata
    )
    .await?;

    tracing::info!(
        enrollment_id = %enrollment_id,
        reference = %reference,
        amount_kobo = plan.amount_kobo,
        "enrollment initiated"
    );

    Ok((StatusCode::CREATED, Json(InitiateEnrollementResponse {
        enrollment_id,
        payment_reference: reference,
        authorization_url: init.authorization_url,
        amount_kobo: plan.amount_kobo,
    })))
}

#[derive(Debug, Deserialize)]
pub struct ManualEnrollmentRequest {
    pub student_user_id: Uuid,
    pub tuition_plan_id: Uuid,
    pub external_reference: Option<String>,
    pub paid_amount_kobo: Option<i64>
}

#[derive(Debug, Serialize)]
pub struct ManualEnrollmentResponse {
    pub enrollment_id: Uuid,
    pub payment_id: Uuid,
    pub starts_at: chrono::DateTime<Utc>,
    pub ends_at: chrono::DateTime<Utc>,
}

pub async fn manual_enrollment(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Json(req): Json<ManualEnrollmentRequest>
) -> AppResult<(StatusCode, Json<ManualEnrollmentResponse>)> {
    if ctx.role.is_admin_or_above() {
        return Err(AppError::Forbidden("admin role required".into()));
    }

    if matches!(ctx.payment_mode, PaymentMode::LektaManaged) {
        return Err(AppError::BadRequest("this workspace uses lekta-managed payments; use the standard enrollment endpoint".into()));
    }

    // Verify target student is a workspace member
    let member = sqlx::query!(
        r#"
        SELECT wm.user_id, wm.role
        FROM workspace_members wm
        WHERE wm.workspace_id = $1 AND wm.user_id = $2 AND wm.status = 'active'
        "#,
        ctx.workspace_id,
        req.student_user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("user is not a workspace member".into()))?;

    if member.role != "student" {
        return Err(AppError::BadRequest("target must be a student".into()));
    }

    // Fetch plan
    let plan = sqlx::query!(
        r#"
        SELECT id, amount_kobo, duration_days, is_active
        FROM tuition_plans
        WHERE id = $1 AND workspace_id = $2 AND deleted_at IS NULL
        "#,
        req.tuition_plan_id,
        ctx.workspace_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("tuition plan is not found".into()))?;

    if !plan.is_active {
        return Err(AppError::BadRequest("plan is not active".into()));
    }

    let now = Utc::now();
    let ends_at = now + Duration::days(plan.duration_days as i64);
    let paid_amount = req.paid_amount_kobo.unwrap_or(plan.amount_kobo);

    let enrollment_id = Uuid::now_v7();
    let payment_id = Uuid::now_v7();
    let reference = format!("lekta_manual_{}", nanoid::nanoid!(10));

    let mut tx = state.db.begin().await?;

    // Create active enrollment
    sqlx::query!(
        r#"
        INSERT INTO enrollments (
            id, workspace_id, student_user_id, tuition_plan_id,
            status, enrollment_source, enrollment_reference,
            starts_at, ends_at
        )
        VALUES ($1, $2, $3, $4, 'active', 'manual', $5, $6, $7)
        "#,
        enrollment_id,
        ctx.workspace_id,
        req.student_user_id,
        plan.id,
        req.external_reference,
        now,
        ends_at
    )
    .execute(&mut *tx)
    .await?;

    // Create successful payment
    sqlx::query!(
        r#"
        INSERT INTO payments (
            id, reference, workspace_id, payer_user_id, enrollment_id,
            payment_purpose, amount_kobo, status, paid_at
        )
        VALUES ($1, $2, $3, $4, $5, 'tuition', $6, 'successful', $7)
        "#,
        payment_id,
        reference,
        ctx.workspace_id,
        req.student_user_id,
        enrollment_id,
        paid_amount,
        now
    )
    .execute( &mut *tx)
    .await?;

    sqlx::query!(
        "UPDATE enrollments SET payment_id = $1 WHERE id = $2",
        payment_id,
        enrollment_id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    tracing::info!(
        enrollment_id = %enrollment_id,
        workspace_id = %ctx.workspace_id,
        student_user_id = %req.student_user_id,
        recorded_by = %ctx.user_id,
        "manual enrollment created"
    );

    Ok((StatusCode::CREATED, Json(ManualEnrollmentResponse {
        enrollment_id,
        payment_id,
        starts_at: now,
        ends_at,
    })))

}