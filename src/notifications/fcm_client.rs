use std::sync::Arc;

use gcp_auth::{CustomServiceAccount, TokenProvider};
use reqwest::Client;
use serde::Serialize;

use crate::error::{AppError, AppResult};

#[derive(Clone)]
pub struct FcmClient {
    http: Client,
    project_id: String,
    service_account: Arc<CustomServiceAccount>
}

#[derive(Debug, Serialize)]
struct FcmMessage<'a> {
    message: FcmMessageBody<'a>
}   

#[derive(Debug, Serialize)]
pub struct FcmMessageBody<'a> {
    pub token: &'a str,
    pub notification: FcmNotification<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
    apns: FcmApns,
}

#[derive(Debug, Serialize)]
struct FcmNotification<'a> {
    pub title: &'a str,
    pub body: &'a str,
}

#[derive(Debug, Serialize)]
struct FcmApns {
    payload: FcmApnsPayload,
}

#[derive(Debug, Serialize)]
struct FcmApnsPayload {
    aps: FcmAps,
}

#[derive(Debug, Serialize)]
struct FcmAps {
    sound: String,
    #[serde(rename = "content_available")]
    content_available: i32,
}

impl FcmClient {
    pub async fn new(http: Client, service_account_json: String, project_id: String) -> AppResult<Self> {
        let service_account = CustomServiceAccount::from_json(&service_account_json).map_err(|e| AppError::Internal(format!("fcm service account: {}",e)))?;

        Ok(Self { http, project_id, service_account: Arc::new(service_account) })
    }

    async fn access_token(&self) -> AppResult<String>{
        let scopes = &["https://www.googleapis.com/auth/firebase.messaging"];
        let token = self.service_account.token(scopes).await.map_err(|e| AppError::Internal(format!("fcm token fetch: {}",e)))?;

        Ok(token.as_str().to_string())
    }

    pub async fn send(
        &self,
        device_token: &str,
        title: &str,
        body: &str,
        data: Option<serde_json::Value>,
    ) -> AppResult<bool> {
        let access_token = self.access_token().await?;
        let url = format!("https://fcm.googleapis.com/v1/projects/{}/messages:send", self.project_id);

        let message = FcmMessage {
            message: FcmMessageBody {
                token: device_token,
                notification: FcmNotification {
                    title,
                    body,
                },
                data,
                apns: FcmApns {
                    payload: FcmApnsPayload {
                        aps: FcmAps {
                            sound: "default".to_string(),
                            content_available: 1,
                        }
                    }
                }
            }
        };

        let resp = self.http
        .post(&url)
        .bearer_auth(access_token)
        .json(&message)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("fcm request failed: {}",e)))?;

        let status = resp.status();
        if status.is_success() {
            return Ok(true);
        }

        let body = resp.text().await.unwrap_or_default();
        if status == 400 || body.contains("UNREGISTERED") || body.contains("INVALID_ARGUMENT") {
            tracing::warn!(device_token_prefix = %device_token[..device_token.len().min(10)], "fcm token invalid, will remove");
             return Ok(false)
        }
        Err(AppError::Internal(format!("fcm token error: {}:{}",status,body)))
    }
}