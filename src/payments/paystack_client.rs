use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Clone)]
pub struct PaystackClient {
    http: Client,
    secret_key: String,
    base_url: String,
}

impl PaystackClient {
    pub fn new(http: Client, secret_key: String, base_url: String) -> Self {
        Self { http, secret_key, base_url }
    }
}

#[derive(Debug, Deserialize)]
struct PaystackResponse<T> {
    status: bool,
    message: String,
    #[serde(default = "Option::default")]
    data: Option<T>,
}

#[derive(Debug, Deserialize)]
pub struct ResolvedAccount {
    pub account_number: String,
    pub account_name: String,
    pub bank_id: i64,
}

impl PaystackClient {
    pub async fn resolve_account (
        &self,
        account_number: &str,
        bank_code: &str,
    ) -> AppResult<ResolvedAccount> {
        let url = format!("{}/bank/resolved", self.base_url);

        let resp = self.http
            .get(&url)
            .bearer_auth(&self.secret_key)
            .query(&[
                ("account_number", account_number),
                ("bank_code", bank_code)
            ])
            .send()
            .await
            .map_err(|e| AppError::Paystack(format!("request failed: {}", e)))?;

        let parsed: PaystackResponse<ResolvedAccount> = resp
            .json()
            .await
            .map_err(|e| AppError::Paystack(format!("decode failed: {}", e)))?;

        if !parsed.status {
            return Err(AppError::Paystack(parsed.message));
        }

        parsed.data.ok_or_else(|| AppError::Paystack("empty response data".into()))
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Bank {
    pub name: String,
    pub code: String,
    pub country: String,
    pub currency: String,
    #[serde(rename = "type")]
    pub bank_type: String,
    pub active: bool,
}

impl PaystackClient {
    pub async fn list_banks(
        &self,
    ) -> AppResult<Vec<Bank>> {
        let url = format!("{}/bank", self.base_url);

        let resp = self.http
        .get(&url)
        .bearer_auth(&self.secret_key)
        .query(&[
            ("country", "nigeria"),
            ("type", "nuban")
        ])
        .send()
        .await
        .map_err(|e| AppError::Paystack(format!("request failed: {}", e)))?;

        let parsed: PaystackResponse<Vec<Bank>> = resp
        .json()
        .await
        .map_err(|e| AppError::Paystack(format!("decode failed: {}", e)))?;

        if !parsed.status {
            return Err(AppError::Paystack(parsed.message));
        }

        Ok(parsed.data.unwrap_or_default())
    }
}

#[derive(Debug, Serialize)]
struct CreateSubaccountBody<'a> {
    business_name: &'a str,
    settlement_bank: &'a str,
    account_number: &'a str,
    percentage_charge: f64
}

#[derive(Debug, Deserialize)]
pub struct Subaccount {
    pub subaccount_code: String,
    pub id: i64,
    pub business_name: String,
    pub account_number: String,
    pub bank: String,
}

impl PaystackClient {
    pub async fn create_subaccount(
        &self,
        business_name: &str,
        bank_code: &str,
        account_number: &str,
        percentage_charge: f64,
    ) -> AppResult<Subaccount> {
        let url = format!("{}/subaccount", self.base_url);

        let body = CreateSubaccountBody {
            business_name,
            settlement_bank: bank_code,
            account_number,
            percentage_charge,
        };

        let resp = self.http
            .post(&url)
            .bearer_auth(&self.secret_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Paystack(format!("request failed: {}",e)))?;

        let parsed: PaystackResponse<Subaccount> = resp
            .json()
            .await
            .map_err(|e| AppError::Paystack(format!("decode failed: {}",e)))?;

        if !parsed.status {
            return Err(AppError::Paystack(parsed.message));
        }

        parsed.data.ok_or_else(|| AppError::Paystack("empty response data".into()))
    }
}

#[derive(Debug, Serialize)]
struct InitTransactionBody<'a> {
    email: &'a str,
    amount: i64,
    reference: &'a str,
    subaccount: Option<&'a str>,
    callback_url: &'a str,
    metadata: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct TransactionInit {
    pub authorization_url: String,
    pub access_code: String,
    pub reference: String,
}

impl PaystackClient {
    pub async fn initialize_transaction(
        &self,
        email: &str,
        amount_kobo: i64,
        reference: &str,
        subaccount: Option<&str>,
        callback_url: &str,
        metadata: serde_json::Value,
    ) -> AppResult<TransactionInit> {
        let url = format!("{}/transaction/initialize", self.base_url);
        
        let body = InitTransactionBody {
            email,
            amount: amount_kobo,
            reference,
            subaccount,
            callback_url,
            metadata,
        };

        let resp = self.http
            .post(&url)
            .bearer_auth(&self.secret_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| AppError::Paystack(format!("request failed: {}",e)))?;

        let parsed: PaystackResponse<TransactionInit> = resp
            .json()
            .await
            .map_err(|e| AppError::Paystack(format!("decode failed: {}",e)))?;

        if !parsed.status {
            return Err(AppError::Paystack(parsed.message));
        }

        parsed.data.ok_or_else(|| AppError::Paystack("empty response data".into()))
    }
}

#[derive(Debug, Deserialize)]
pub struct TransactionStatus {
    pub reference: String,
    pub amount: i64,
    pub status: String, // "success", "failed", "abandoned"
    pub gateway_response: String,
    pub paid_at: Option<String>,
    pub fees: Option<i64>,
    pub metadata: Option<serde_json::Value>,
}

impl PaystackClient {
    pub async fn verify_transaction(
        &self,
        reference: &str
    ) -> AppResult<TransactionStatus> {
        let url = format!("{}/transaction/verify/{}", self.base_url, reference);

        let resp = self.http
            .get(&url)
            .bearer_auth(&self.secret_key)
            .send()
            .await
            .map_err(|e| AppError::Paystack(format!("request failed: {}",e)))?;

        let parsed: PaystackResponse<TransactionStatus> = resp
            .json()
            .await
            .map_err(|e| AppError::Paystack(format!("decode failed: {}",e)))?;

        if !parsed.status {
            return Err(AppError::Paystack(parsed.message));
        }

        parsed.data.ok_or_else(|| AppError::Paystack("empty response data".into()))
    }
}


