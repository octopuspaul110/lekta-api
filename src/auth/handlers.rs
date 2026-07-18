
use axum::{Json, extract::State};
use chrono::{DateTime, Duration, Utc};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{auth::{extractor::AuthUser, jwt::encode_access_token, password::{hash_password, verify_password}, refresh_token::{generate_refresh_token, hash_refresh_token}}, error::{AppError, AppResult}, state::AppState};

#[derive(Debug, Deserialize, Validate)]
pub struct RegisterRequest {
    #[validate(email)]
    pub email: String,

    #[validate(length(min = 2, max = 128))]
    pub password: String,

    #[validate(length(min = 2, max = 100))]
    pub full_name: String,

    pub timezone: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UserResponse {
    pub id: Uuid,
    pub email: String,
    pub full_name: String,
    pub email_verified: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct LoginRequest {
    #[validate(email)]
    pub email: String,
    pub password: String,
    pub device_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,
    pub user: UserResponse,
}

#[derive(Debug, Deserialize)]
pub struct RefreshRequest{
    pub refresh_token: String,
}

#[derive(Debug, Serialize)]
pub struct RefreshResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64
}

pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> AppResult<(StatusCode, Json<UserResponse>)>{
    req.validate()?;

    let email_lower = req.email.to_lowercase();

    let existing = sqlx::query!(
        "
        SELECT id 
        FROM users
        WHERE lower(email) = $1 AND deleted_at IS NULL
        ",
        email_lower
    )
    .fetch_optional(&state.db)
    .await?;

    if existing.is_some() {
        return Err(AppError::Conflict("Email already registered".into()));
    }

    let password_hash = hash_password(&req.password)?;

    let user_id = Uuid::now_v7();

    let user = sqlx::query!(
        "
        INSERT INTO users (id, email, password_hash, full_name, timezone)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, email, full_name, email_verified, created_at
        ",
        user_id,
        email_lower,
        password_hash,
        req.full_name,
        req.timezone.unwrap_or_else(|| "Africa/Lagos".into())
    )
    .fetch_one(&state.db)
    .await?;

    // Generate email verification token
    let verification_token = generate_refresh_token();
    let verification_hash = hash_refresh_token(&verification_token);
    let expires_at = Utc::now() + Duration::hours(48);

    sqlx::query!(
        "INSERT INTO email_verification_tokens (user_id, token_hash, expires_at)
        VALUES ($1, $2, $3)",
        user_id,
        verification_hash,
        expires_at
    )
    .execute(&state.db)
    .await?;

    if !state.config.environment.is_production() {
        tracing::info!(
            user_id = %user_id,
            verification_token = %verification_token,
            "verification token generated (TODO: send via email)"
        );
    }

    Ok((
        StatusCode::CREATED,
        Json(UserResponse { 
            id: user.id, 
            email: user.email, 
            full_name: user.full_name, 
            email_verified: user.email_verified, 
            created_at: user.created_at 
        })
    ))
}

pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>
)-> AppResult<Json<AuthResponse>>{
    req.validate()?;

    let email_lower = req.email.to_lowercase();
    
    let user = sqlx::query!(
        "
            SELECT id, email, password_hash, full_name, email_verified, created_at
            FROM users
            WHERE lower(email) = $1 AND deleted_at IS NULL
        ",
        email_lower
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::Unauthorized("Invalid credentials".into()))?;

    // Google-only accounts have no password hash
    let password_hash = user   
        .password_hash
        .ok_or_else(|| AppError::Unauthorized("Use Google sign-in for this account".into()))?;

    if !verify_password(&req.password, &password_hash){
        return Err(AppError::Unauthorized("Invalid credentials".into()));
    }

    // Generate tokens
    let access_token = encode_access_token(user.id, &state.config)?;
    let refresh_raw = generate_refresh_token();
    let refresh_hash = hash_refresh_token(&refresh_raw);
    let expires_at = Utc::now() + Duration::days(state.config.refresh_token_expiry_days);

    sqlx::query!(
        "
        INSERT INTO refresh_tokens (user_id, token_hash, device_name, expires_at)
        VALUES ($1, $2, $3, $4)
        ",
        user.id,
        refresh_hash,
        req.device_name,
        expires_at,
    )
    .execute(&state.db)
    .await?;

    sqlx::query!(
        "
        UPDATE users SET last_login_at = NOW() WHERE id = $1
        ",
        user.id
    )
    .execute(&state.db)
    .await?;

    Ok(
        Json(AuthResponse { 
            access_token, 
            refresh_token: refresh_raw, 
            expires_at: state.config.access_token_expiry_seconds, 
            user: UserResponse { 
                id: user.id, 
                email: user.email, 
                full_name: user.full_name, 
                email_verified: user.email_verified, 
                created_at: user.created_at
            }
        })
    )
}

pub async fn refresh(
    State(state): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> AppResult<Json<RefreshResponse>> {
    let token_hash = hash_refresh_token(&req.refresh_token);

    let token_row = sqlx::query!(
        "
            SELECT id, user_id, revoked, expires_at
            FROM refresh_tokens
            WHERE token_hash = $1
        ",
        token_hash
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::Unauthorized("invalid refresh token".into()))?;

    // check that the token is expired
    if token_row.expires_at < Utc::now() {
        return Err(AppError::Unauthorized("Refresh token expired".into()));
    }

    //check that the token is revoked, meaning it has been used before, perhaps by theft
    if token_row.revoked {
        // CRITICAL: revoke all of the user's refresh tokens
        sqlx::query!(
            "
                UPDATE refresh_tokens
                SET revoked = TRUE, revoked_at = NOW()
                WHERE user_id = $1 AND revoked = FALSE
            ",
            token_row.user_id
        )
        .execute(&state.db)
        .await?;

        tracing::warn!(
            user_id = %token_row.user_id,
            "Revoked refresh token reused - all tokens revoked"
        );

        return Err(AppError::Unauthorized("Token reuse detected".into()));
    }

    // Rotate good tokens
    let new_refresh_raw = generate_refresh_token();
    let new_refresh_hash = hash_refresh_token(&new_refresh_raw);
    let new_expires_at = Utc::now() + Duration::days(state.config.refresh_token_expiry_days);

    // Insert new tokens
    let new_token = sqlx::query!(
        "
        INSERT INTO refresh_tokens (user_id, token_hash, expires_at)
        VALUES ($1, $2, $3)
        RETURNING id
        ",
        token_row.user_id,
        new_refresh_hash,
        new_expires_at
    )
    .fetch_one(&state.db)
    .await?;

    sqlx::query!(
        "
            UPDATE refresh_tokens
            SET revoked = TRUE, revoked_at = NOW(), replaced_by_token_id = $1
            WHERE id = $2
        ",
        new_token.id,
        token_row.id
    )
    .execute(&state.db)
    .await?;

    let access_token = encode_access_token(token_row.user_id, &state.config)?;

    Ok(Json(
        RefreshResponse {
            access_token,
            refresh_token: new_refresh_raw,
            expires_in: state.config.access_token_expiry_seconds,
        }
    ))
}

#[derive(Deserialize)]
pub struct LogoutRequest {
    refresh_token: Option<String>,
}

pub async fn logout(
    State(state): State<AppState>,
    _auth: AuthUser,
    Json(req): Json<LogoutRequest>
) -> AppResult<StatusCode> {
    if let Some(token) = req.refresh_token {
        let token_hash = hash_refresh_token(&token);
        sqlx::query!(
            "
                UPDATE refresh_tokens SET revoked = TRUE, revoked_at = NOW()
                WHERE token_hash = $1 AND revoked = FALSE
            ",
            token_hash
        )
        .execute(&state.db)
        .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn logout_all(
    State(state): State<AppState>,
    auth: AuthUser
) -> AppResult<StatusCode>{
    sqlx::query!(
        "
            UPDATE refresh_tokens SET revoked = TRUE, revoked_at = NOW()
            WHERE user_id = $1 AND revoked = FALSE
        ",
        auth.user_id
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize,Validate)]
pub struct ForgotPasswordRequest {
    #[validate(email)]
    pub email: String,
}

pub async fn forgot_password(
    State(state): State<AppState>,
    Json(req): Json<ForgotPasswordRequest>,
)-> AppResult<StatusCode> {
    req.validate()?;
    let email_lower = req.email.to_lowercase();

    let user = sqlx::query!(
        "
            SELECT id FROM users WHERE lower(email) = $1 AND deleted_at IS NULL
        ",
        email_lower
    )
    .fetch_optional(&state.db)
    .await?;

    if let Some(user) = user {
        let reset_token = generate_refresh_token();
        let reset_hash = hash_refresh_token(&reset_token);
        let expires_at = Utc::now() + Duration::hours(1);

        sqlx::query!(
            "
                INSERT INTO password_reset_tokens (user_id, token_hash, expires_at)
                VALUES ($1, $2, $3)
            ",
            user.id, 
            reset_hash,
            expires_at
        )
        .execute(&state.db)
        .await?;

        if !state.config.environment.is_production() {
            tracing::info!(
                user_id = %user.id,
                reset_token = %reset_token,
                "password reset token (DEV ONLY)"
            );
        }
        
        // email is to be sent via ses
    }

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, Validate)]
pub struct ResetPasswordRequest {
    pub token: String,
    #[validate(length(min = 8, max = 128))]
    pub new_password: String,
}

pub async fn reset_password(
    State(state): State<AppState>,
    Json(req): Json<ResetPasswordRequest>
) -> AppResult<StatusCode> {
    req.validate()?;
    let token_hash = hash_refresh_token(&req.token);

    let token_row = sqlx::query!(
        "
            SELECT id, user_id, used, expires_at
            FROM password_reset_tokens WHERE token_hash = $1
        ",
        token_hash
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::Unauthorized("Invalid reset token".into()))?;

    if token_row.used {
        return Err(AppError::Unauthorized("Invalid reset token".into()))?;
    }

    if token_row.expires_at < Utc::now() {
        return Err(AppError::Unauthorized("Reset token already used".into()))?;
    }

    let new_hash = hash_password(&req.new_password)?;
    
    let mut tx = state.db.begin().await?;

    sqlx::query!(
        "UPDATE users SET password_hash = $1 WHERE id = $2",
        new_hash, token_row.user_id
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!(
        "UPDATE password_reset_tokens SET used = TRUE, used_at = NOW() WHERE id = $1",
        token_row.id
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!(
        "
            UPDATE refresh_tokens SET revoked = TRUE, revoked_at = NOW()
            WHERE user_id = $1 AND revoked = FALSE
        ",
        token_row.user_id
    )
    .execute(&state.db)
    .await?;

    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct VerifyEmailRequest {
    pub token: String,
}

pub async fn verify_email(
    State(state): State<AppState>,
    Json(req): Json<VerifyEmailRequest>
) -> AppResult<StatusCode> {
    let token_hash = hash_refresh_token(&req.token);

    let token_row = sqlx::query!(
        "
            SELECT id, user_id, used, expires_at
            FROM email_verification_tokens
            WHERE token_hash = $1
        ",
        token_hash
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::Unauthorized("Invalid verification token".into()))?;

    if token_row.used {
        return Err(AppError::Unauthorized("verification token already used".into()))?;
    }

    if token_row.expires_at < Utc::now() {
        return Err(AppError::Unauthorized("verification token expired".into()))?;
    }

    let mut tx = state.db.begin().await?;

    sqlx::query!(
        "
            UPDATE users SET email_verified = TRUE WHERE id = $1
        ",
        token_row.user_id
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!(
        "
            UPDATE email_verification_tokens SET used = TRUE, used_at = NOW() WHERE id = $1
        ",
        token_row.id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}