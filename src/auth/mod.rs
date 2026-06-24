use axum::{Router, routing::post};

use crate::state::AppState;

pub mod jwt;
pub mod refresh_token;
pub mod password;
pub mod extractor;
pub mod handlers;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/register", post(handlers::register))
        .route("/login", post(handlers::login))
        .route("/refresh", post(handlers::refresh))
}