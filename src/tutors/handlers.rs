use axum::{Json, extract::{Path, State}};
use chrono::{DateTime, Utc};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{error::{AppError, AppResult}, state::AppState, workspaces::extractor::WorkspaceContext};

#[derive(Debug, Serialize)]
pub struct TutorProfileResponse {
    pub id: Uuid,
    pub user_id: Uuid,
    pub workspace_id: Uuid,
    pub full_name: String,
    pub email: String,
    pub avatar_key: Option<String>,
    pub bio: Option<String>,
    pub credentials: Vec<String>,
    pub subjects: Vec<String>,
    pub years_experience: Option<i32>,
    pub profile_photo_key: Option<String>,
    pub verified_by_workspace: bool,
    pub verified_at: Option<DateTime<Utc>>,
    pub avg_rating: Option<f64>,
    pub rating_count: i32,
    pub created_at: DateTime<Utc>,
}

pub async fn list_tutors(
    State(state): State<AppState>,
    ctx: WorkspaceContext
) -> AppResult<Json<Vec<TutorProfileResponse>>>{
    let rows = sqlx::query!(
        r#"
        SELECT tp.id, tp.user_id, tp.workspace_id, tp.bio, tp.credentials, tp.subjects,
            tp.years_experience, tp.profile_photo_key, tp.verified_by_workspace,
            tp.verified_at, tp.avg_rating::FLOAT8 as avg_rating, tp.rating_count, tp.created_at,
            u.full_name, u.email, u.avatar_key
        FROM tutor_profiles tp
        JOIN users u ON u.id = tp.user_id
        JOIN workspace_members wm ON wm.user_id = tp.user_id AND wm.workspace_id = tp.workspace_id
        WHERE tp.workspace_id = $1
          AND wm.status = 'active'
          AND wm.role IN ('proprietor', 'admin', 'tutor')
        ORDER BY tp.verified_by_workspace DESC, tp.avg_rating DESC NULLS LAST, tp.created_at ASC
        "#,
        ctx.workspace_id
    )
    .fetch_all(&state.db)
    .await?;

    let tutors = rows.into_iter().map(|r| TutorProfileResponse {
        id: r.id,
        user_id: r.user_id,
        workspace_id: r.workspace_id,
        full_name: r.full_name,
        email: r.email,
        avatar_key: r.avatar_key,
        bio: r.bio,
        credentials: r.credentials,
        subjects: r.subjects,
        years_experience: r.years_experience,
        profile_photo_key: r.profile_photo_key,
        verified_by_workspace: r.verified_by_workspace,
        verified_at: r.verified_at,
        avg_rating: r.avg_rating,
        rating_count: r.rating_count,
        created_at: r.created_at,
    }).collect();

    Ok(Json(tutors))
}

pub async fn get_tutor_profile(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Path((_slug, tutor_user_id)): Path<(String, Uuid)>,
) -> AppResult<Json<TutorProfileResponse>> {
    let row = sqlx::query!(
        r#"
        SELECT
            tp.id, tp.user_id, tp.workspace_id, tp.bio, tp.credentials, tp.subjects,
            tp.years_experience, tp.profile_photo_key, tp.verified_by_workspace,
            tp.verified_at, tp.avg_rating::FLOAT8 as avg_rating, tp.rating_count, tp.created_at,
            u.full_name, u.email, u.avatar_key
        FROM tutor_profiles tp
        JOIN users u ON u.id = tp.user_id
        WHERE tp.workspace_id = $1 AND tp.user_id = $2
        "#,
        ctx.workspace_id,
        tutor_user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("tutor profile not found".into()))?;

    Ok(Json(TutorProfileResponse {
        id: row.id,
        user_id: row.user_id,
        workspace_id: row.workspace_id,
        full_name: row.full_name,
        email: row.email,
        avatar_key: row.avatar_key,
        bio: row.bio,
        credentials: row.credentials,
        subjects: row.subjects,
        years_experience: row.years_experience,
        profile_photo_key: row.profile_photo_key,
        verified_by_workspace: row.verified_by_workspace,
        verified_at: row.verified_at,
        avg_rating: row.avg_rating,
        rating_count: row.rating_count,
        created_at: row.created_at,
    }))
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpsertTutorProfileRequest {
    #[validate(length(max = 2000))]
    pub bio: Option<String>,

    pub credentials: Option<Vec<String>>,
    pub subjects: Option<Vec<String>>,

    #[validate(range(min = 0, max = 60))]
    pub years_experience: Option<i32>,
}

pub async fn upsert_tutor_profile(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Path((_slug, tutor_user_id)): Path<(String, Uuid)>,
    Json(req): Json<UpsertTutorProfileRequest>,
) -> AppResult<Json<TutorProfileResponse>> {
    req.validate()?;

    // Permission: self OR admin+
    let is_self = tutor_user_id == ctx.user_id;
    if !is_self && !ctx.role.is_admin_or_above() {
        return Err(AppError::Forbidden("self or admin only".into()));
    }

    // Target must be a workspace member with tutor+ role
    let member = sqlx::query!(
        r#"
        SELECT role FROM workspace_members
        WHERE workspace_id = $1 AND user_id = $2 AND status = 'active'
        "#,
        ctx.workspace_id,
        tutor_user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("user not in workspace".into()))?;

    if !matches!(member.role.as_str(), "proprietor" | "admin" | "tutor") {
        return Err(AppError::BadRequest("target must have tutor role or higher".into()));
    }

    // Upsert
    sqlx::query!(
        r#"
        INSERT INTO tutor_profiles (
            user_id, workspace_id, bio, credentials, subjects, years_experience
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (user_id, workspace_id) DO UPDATE SET
            bio = COALESCE(EXCLUDED.bio, tutor_profiles.bio),
            credentials = COALESCE(EXCLUDED.credentials, tutor_profiles.credentials),
            subjects = COALESCE(EXCLUDED.subjects, tutor_profiles.subjects),
            years_experience = COALESCE(EXCLUDED.years_experience, tutor_profiles.years_experience)
        "#,
        tutor_user_id,
        ctx.workspace_id,
        req.bio,
        req.credentials.as_deref(),
        req.subjects.as_deref(),
        req.years_experience
    )
    .execute(&state.db)
    .await?;

    // Return the updated profile
    let row = sqlx::query!(
        r#"
        SELECT
            tp.id, tp.user_id, tp.workspace_id, tp.bio, tp.credentials, tp.subjects,
            tp.years_experience, tp.profile_photo_key, tp.verified_by_workspace,
            tp.verified_at, tp.avg_rating::FLOAT8 as avg_rating, tp.rating_count, tp.created_at,
            u.full_name, u.email, u.avatar_key
        FROM tutor_profiles tp
        JOIN users u ON u.id = tp.user_id
        WHERE tp.workspace_id = $1 AND tp.user_id = $2
        "#,
        ctx.workspace_id,
        tutor_user_id
    )
    .fetch_one(&state.db)
    .await?;

    Ok(Json(TutorProfileResponse {
        id: row.id,
        user_id: row.user_id,
        workspace_id: row.workspace_id,
        full_name: row.full_name,
        email: row.email,
        avatar_key: row.avatar_key,
        bio: row.bio,
        credentials: row.credentials,
        subjects: row.subjects,
        years_experience: row.years_experience,
        profile_photo_key: row.profile_photo_key,
        verified_by_workspace: row.verified_by_workspace,
        verified_at: row.verified_at,
        avg_rating: row.avg_rating,
        rating_count: row.rating_count,
        created_at: row.created_at,
    }))
}

pub async fn verify_tutor(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Path((_slug, tutor_user_id)): Path<(String, Uuid)>,
) -> AppResult<StatusCode> {
    if !ctx.role.is_admin_or_above() {
        return Err(AppError::Forbidden("admin role required".into()));
    }

    let result = sqlx::query!(
        r#"
        UPDATE tutor_profiles
        SET verified_by_workspace = TRUE,
            verified_at = NOW(),
            verified_by_user_id = $1
        WHERE workspace_id = $2 AND user_id = $3
        "#,
        ctx.user_id,
        ctx.workspace_id,
        tutor_user_id
    )
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("tutor profile not found".into()));
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn tutor_photo_upload_url(
    State(_state): State<AppState>,
    ctx: WorkspaceContext,
    Path((_slug, tutor_user_id)): Path<(String, Uuid)>,
) -> AppResult<StatusCode> {
    // TODO: generate presigned S3 URL once S3 client is wired
    tracing::info!(
        workspace_id = %ctx.workspace_id,
        tutor_user_id = %tutor_user_id,
        "photo upload URL requested (TODO: S3)"
    );
    Err(AppError::ServiceUnavailable("photo upload not yet implemented".into()))
}