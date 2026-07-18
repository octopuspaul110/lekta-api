use axum::Json;
use axum::response::{IntoResponse};
use axum::http::StatusCode;
use serde_json::json;
use tracing;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Not Found: {0}")]
    NotFound(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Unprocessable: {0}")]
    UnprocessableEntity(String),

    #[error("Rate limited: {0}")] // 429 rate limiting with Governor
    TooManyRequests(String),

    #[error("PaymentRequired: {0}")]
    PaymentRequired(String),

    #[error("Service Unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Database Error")]
    Database(#[from] sqlx::Error),

    #[error("Redis error")]
    Redis(#[from] redis::RedisError),

    #[error("HTTP client error")]
    Reqwest(#[from] reqwest::Error),

    #[error("Serialization error")]
    Serialization(#[from] serde_json::Error),

    #[error("Validation failed")]
    Validation(#[from] validator::ValidationErrors),

    #[error("Paystack error: {0}")]
    Paystack(String),

    #[error("Anthropic error: {0}")]
    Anthropic(String),

    #[error("AWS error: {0}")]
    Aws(String)
}

impl AppError {
    fn status_and_code(&self) -> (StatusCode, &'static str) {
        match self {
            AppError::BadRequest(_) => (StatusCode::BAD_REQUEST, "BAD_REQUEST"),
            AppError::Unauthorized(_) => (StatusCode::UNAUTHORIZED, "UNAUTHORIZED"),
            AppError::Forbidden(_) => (StatusCode::FORBIDDEN, "FORBIDDEN"),
            AppError::NotFound(_) => (StatusCode::NOT_FOUND, "NOT_FOUND"),
            AppError::Conflict(_) => (StatusCode::CONFLICT, "CONFLICT"),
            AppError::TooManyRequests(_) => (StatusCode::TOO_MANY_REQUESTS, "TooManyRequests"),
            AppError::PaymentRequired(_) => (StatusCode::PAYMENT_REQUIRED, "PAYMENT_REQUIRED"),
            AppError::ServiceUnavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "SERVICE_UNAVAILABLE"),
            AppError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_SERVER_ERROR"),
            AppError::Database(_) => {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR"
                )
            },
            AppError::Redis(_) => {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR"
                )
            },
            AppError::Serialization(_) => {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR"
                )
            },
            AppError::UnprocessableEntity(_) | AppError::Validation(_) => 
                (StatusCode::UNPROCESSABLE_ENTITY,"UNPROCESSABLE_ENTITY"),
            AppError::Reqwest(_) |
            AppError::Paystack(_) |
            AppError::Anthropic(_) |
            AppError::Aws(_) => (StatusCode::BAD_GATEWAY, "UPSTREAM_ERROR"),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, code) = self.status_and_code();

        let message = match self {
            AppError::Database(_) |
            AppError::Redis(_) |
            AppError::Reqwest(_) | 
            AppError::Serialization(_) | 
            AppError::Internal(_) |
            AppError::Paystack(_) |
            AppError::Anthropic(_) |
            AppError::Aws(_) => {
                tracing::info!(error = ?self, "internal error");
                "An internal error occurred, please try again.".to_string()
            }
            _ => self.to_string(),
        };

        let body = Json(
            json!({
                "error" : {
                    "code" : code,
                    "message" : message
                }
            })
        );
        (status, body).into_response()
    }
}