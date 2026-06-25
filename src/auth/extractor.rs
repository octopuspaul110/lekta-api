use axum::extract::FromRequestParts;
use axum::http::header;
use uuid::Uuid;
use axum::http::request::Parts;

use crate::{auth::jwt::decode_access_token, error::AppError, state::AppState};

#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: Uuid,
    pub email: String,
    pub email_verified: bool,
    pub is_platform_admin: bool
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) ->  Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get(header::AUTHORIZATION)
            .ok_or_else(|| AppError::Unauthorized("Missing authorization header".into()))?;

        let auth_str = auth_header
            .to_str()
            .map_err(|_| AppError::Unauthorized("Invalid authorization header encoding".into()))?;

        let token = auth_str
            .strip_prefix("Bearer ")
            .ok_or_else(|| AppError::Unauthorized("Invalid authorization scheme".into()))?;

        let claims = decode_access_token(token, &state.config)?;

        let user_id = Uuid::parse_str(&claims.sub)
            .map_err(|_| AppError::Unauthorized("Invalid token subject".into()))?;

        let user = sqlx::query!(
            "
            SELECT id, email, email_verified, is_platform_admin 
            FROM users
            WHERE id = $1 AND deleted_at IS NULL
            ",
            user_id
        )
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::Unauthorized("User not found".into()))?;
        
        Ok(AuthUser {
            user_id: user.id,
            email: user.email,
            email_verified: user.email_verified,
            is_platform_admin: user.is_platform_admin
        })
    }
}

pub struct OptionalAuthUser(pub Option<AuthUser>);

impl FromRequestParts<AppState> for OptionalAuthUser{
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth = AuthUser::from_request_parts(parts,state).await.ok();
        Ok(OptionalAuthUser(auth))
    }
}