use axum::{Json, extract::{Path, State}};
use chrono::{DateTime, Utc};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{error::{AppError, AppResult}, state::AppState, workspaces::extractor::WorkspaceContext};

#[derive(Debug, Serialize)]
pub struct RatingResponse {
    pub id: Uuid,
    pub tutor_profile_id: Uuid,
    pub student_user_id: Uuid,
    pub student_full_name: String,
    pub rating: i32,
    pub comment: Option<String>,
    pub class_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateRatingRequest {
    #[validate(range(min = 1, max = 5))]
    pub rating: i32,

    #[validate(length(max = 1000))]
    pub comment: Option<String>,

    pub class_id: Option<Uuid>,
}

pub async fn create_rating(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Path((_slug, tutor_user_id)): Path<(String, Uuid)>,
    Json(req): Json<CreateRatingRequest>,
) -> AppResult<(StatusCode, Json<RatingResponse>)> {
    req.validate()?;

    // Only students can rate
    let student_role = matches!(ctx.role,
        crate::workspaces::types::WorkspaceRole::Student
    );
    if !student_role {
        return Err(AppError::Forbidden("only students can rate tutors".into()));
    }

    // Fetch tutor profile
    let profile = sqlx::query!(
        r#"
        SELECT id FROM tutor_profiles
        WHERE user_id = $1 AND workspace_id = $2
        "#,
        tutor_user_id,
        ctx.workspace_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("tutor profile not found".into()))?;

    // If class_id provided, verify it belongs to this workspace and this tutor
    if let Some(class_id) = req.class_id {
        let class = sqlx::query!(
            r#"
            SELECT 1 as "exists!" FROM classes
            WHERE id = $1 AND workspace_id = $2 AND tutor_user_id = $3
            "#,
            class_id,
            ctx.workspace_id,
            tutor_user_id
        )
        .fetch_optional(&state.db)
        .await?;

        if class.is_none() {
            return Err(AppError::BadRequest("class not found or not taught by this tutor".into()));
        }
    }

    let rating_id = Uuid::now_v7();

    let mut tx = state.db.begin().await?;

    // Upsert rating (student can update their existing rating)
    let row = sqlx::query!(
        r#"
        INSERT INTO tutor_ratings (
            id, tutor_profile_id, student_user_id, workspace_id, rating, comment, class_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (tutor_profile_id, student_user_id, class_id) DO UPDATE
        SET rating = EXCLUDED.rating,
            comment = EXCLUDED.comment
        RETURNING id, tutor_profile_id, student_user_id, rating, comment, class_id, created_at
        "#,
        rating_id,
        profile.id,
        ctx.user_id,
        ctx.workspace_id,
        req.rating,
        req.comment,
        req.class_id
    )
    .fetch_one(&mut *tx)
    .await?;

    // Recompute aggregate on tutor_profiles
    sqlx::query!(
        r#"
        UPDATE tutor_profiles
        SET avg_rating = (
            SELECT AVG(rating)::NUMERIC(3,2) FROM tutor_ratings WHERE tutor_profile_id = $1
        ),
        rating_count = (
            SELECT COUNT(*) FROM tutor_ratings WHERE tutor_profile_id = $1
        )
        WHERE id = $1
        "#,
        profile.id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    let student = sqlx::query!(
        "SELECT full_name FROM users WHERE id = $1",
        ctx.user_id
    )
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(RatingResponse {
        id: row.id,
        tutor_profile_id: row.tutor_profile_id,
        student_user_id: row.student_user_id,
        student_full_name: student.full_name,
        rating: row.rating,
        comment: row.comment,
        class_id: row.class_id,
        created_at: row.created_at,
    })))
}

pub async fn delete_rating(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Path((_slug, rating_id)): Path<(String, Uuid)>,
) -> AppResult<StatusCode> {
    let rating = sqlx::query!(
        r#"
        SELECT student_user_id, tutor_profile_id FROM tutor_ratings
        WHERE id = $1 AND workspace_id = $2
        "#,
        rating_id,
        ctx.workspace_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("rating not found".into()))?;

    // Only owner or workspace admin can delete
    let is_owner = rating.student_user_id == ctx.user_id;
    let is_admin = ctx.role.is_admin_or_above();

    if !is_owner && !is_admin {
        return Err(AppError::Forbidden("owner or admin only".into()));
    }

    let mut tx = state.db.begin().await?;

    sqlx::query!("DELETE FROM tutor_ratings WHERE id = $1", rating_id)
        .execute(&mut *tx)
        .await?;

    sqlx::query!(
        r#"
        UPDATE tutor_profiles
        SET avg_rating = (
            SELECT AVG(rating)::NUMERIC(3,2) FROM tutor_ratings WHERE tutor_profile_id = $1
        ),
        rating_count = (
            SELECT COUNT(*) FROM tutor_ratings WHERE tutor_profile_id = $1
        )
        WHERE id = $1
        "#,
        rating.tutor_profile_id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_ratings(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Path((_slug, tutor_user_id)): Path<(String, Uuid)>,
) -> AppResult<Json<Vec<RatingResponse>>> {
    let rows = sqlx::query!(
        r#"
        SELECT
            tr.id, tr.tutor_profile_id, tr.student_user_id, tr.rating, tr.comment,
            tr.class_id, tr.created_at,
            u.full_name as student_full_name
        FROM tutor_ratings tr
        JOIN tutor_profiles tp ON tp.id = tr.tutor_profile_id
        JOIN users u ON u.id = tr.student_user_id
        WHERE tp.user_id = $1 AND tp.workspace_id = $2
        ORDER BY tr.created_at DESC
        "#,
        tutor_user_id,
        ctx.workspace_id
    )
    .fetch_all(&state.db)
    .await?;

    let ratings = rows.into_iter().map(|r| RatingResponse {
        id: r.id,
        tutor_profile_id: r.tutor_profile_id,
        student_user_id: r.student_user_id,
        student_full_name: r.student_full_name,
        rating: r.rating,
        comment: r.comment,
        class_id: r.class_id,
        created_at: r.created_at,
    }).collect();

    Ok(Json(ratings))
}