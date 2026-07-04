use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::{auth::extractor::AuthUser, error::AppResult, payments::paystack_client::Bank, state::AppState};

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
        account_name: resolved.account_name 
    }))
}