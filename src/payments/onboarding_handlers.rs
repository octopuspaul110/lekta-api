use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::{auth::extractor::AuthUser, error::{AppError, AppResult}, payments::paystack_client::Bank, state::AppState, workspaces::{extractor::WorkspaceContext, types::{PaymentMode, WorkspaceRole}}};

pub async fn list_banks(
    State(state): State<AppState>,
    _auth: AuthUser
) -> AppResult<Json<Vec<Bank>>> {
    let banks = state.paystack.list_banks().await?;
    Ok(Json(banks))
}

#[derive(Debug, Deserialize, Validate)]
pub struct ResolveAccountRequest {
    #[validate(length(min = 3, max = 10))]
    pub bank_code: String,

    #[validate(length(equal = 10))]
    pub account_number: String,
}

#[derive(Debug, Serialize)]
pub struct ResolveAccountResponse {
    pub account_number: String,
    pub account_name: String
}

pub async fn resolve_account(
    State(state): State<AppState>,
    _auth: AuthUser,
    Json(req): Json<ResolveAccountRequest>
) -> AppResult<Json<ResolveAccountResponse>> {
    req.validate()?;

    let resolved = state.paystack.resolve_account(&req.account_number, &req.bank_code)
    .await?;

    Ok(Json(ResolveAccountResponse { 
        account_number: resolved.account_number, 
        account_name: resolved.account_name, 
    }))
}

#[derive(Debug, Deserialize, Validate)]
pub struct PaystackOnboardingRequest {
    #[validate(length(min = 3, max = 10))]
    pub bank_code: String,

    #[validate(length(equal = 10))]
    pub account_number: String,

    #[validate(length(min = 2, max = 100))]
    pub business_name: String,
}

#[derive(Debug, Serialize)]
pub struct PaystackOnboardingResponse {
    pub subaccount_code: String,
    pub subaccoubt_status: String,
    pub resolved_account_number: String,
}

pub async fn onboard_paystack(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Json(req): Json<PaystackOnboardingRequest>,
) -> AppResult<Json<PaystackOnboardingResponse>> {
    req.validate()?;

    // Only proprietor can onboard payment
    if matches!(ctx.role,WorkspaceRole::Proprietor) {
        return Err(AppError::Forbidden("proprietor only".into()));
    }

    // cannot onboard if payment is external
    if !matches!(ctx.payment_mode, PaymentMode::External) {
        return  Err(AppError::BadRequest("this workspace uses external payments".into()));
    }

    // Verify the account exists and get the resolved name
    let resolved = state.paystack
        .resolve_account(&req.account_number, &req.bank_code)
        .await?;

    // get platform_fee_basis_points from workspace
    let workspace = sqlx::query!(
        r#"
        SELECT platform_fee_basis_points, paystack_subaccount_code
        FROM workspaces 
        WHERE id = $1
        "#,
        ctx.workspace_id
    )
    .fetch_one(&state.db)
    .await?;

    // if subaccount already exists, dont create a new one
    if workspace.paystack_subaccount_code.is_some() {
        return Err(AppError::Conflict("paystack subaccount already onboarded".into()));
    }

    // Convert basis points to percentage (150 bp = 1.5%)
    let percentage_charge = workspace.platform_fee_basis_points as f64 / 100.0;

    // create subaccount at paystack
    let subaccount = state.paystack
        .create_subaccount(
            &req.business_name, 
            &req.bank_code, 
            &req.account_number, 
            percentage_charge
        )
        .await?;

    // Save on workspace
    sqlx::query!(
        r#"
        UPDATE workspaces
        SET paystack_subaccount_code = $1,
            paystack_subaccount_status = 'active'
        WHERE id = $2
        "#,
        subaccount.subaccount_code,
        ctx.workspace_id
    )
    .execute(&state.db)
    .await?;

    tracing::info!(
        workspace_id = %ctx.workspace_id,
        subaccount_code = %subaccount.subaccount_code,
        "paystack subaccount onboarded"
    );

    Ok(Json(PaystackOnboardingResponse { 
        subaccount_code: subaccount.subaccount_code, 
        subaccoubt_status: "active".to_string(), 
        resolved_account_number: resolved.account_name 
    }))
}