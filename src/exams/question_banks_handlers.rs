use axum::{Json, extract::{Path, Query, State}};
use chrono::{DateTime, Utc};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{auth::extractor::AuthUser, error::{AppError, AppResult}, exams::types::ExamType, state::AppState, workspaces::extractor::WorkspaceContext};

#[derive(Debug, Serialize)]
pub struct QuestionBankResponse {
    pub id: Uuid,
    pub owner_type: String,
    pub name: String,
    pub description: Option<String>,
    pub subject: String,
    pub exam_type: ExamType,
    pub language: String,
    pub is_published: bool,
    pub question_count: i32,
    pub created_by_user_id: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct ListBanksQuery {
    pub workspace_id: Option<Uuid>,
    pub subject: Option<String>,
    pub exam_type: Option<ExamType>
}

pub async fn list_banks(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<ListBanksQuery>,
) -> AppResult<Json<Vec<QuestionBankResponse>>> {
    let exam_type_str = q.exam_type.map(|t| serde_json::to_value(&t).unwrap().as_str().unwrap().to_string());

    let rows = sqlx::query!(
        r#"
        SELECT qb.id, qb.owner_type, qb.owner_workspace_id, qb.name, qb.description,
               qb.subject, qb.exam_type, qb.language, qb.is_published,
               qb.question_count, qb.created_by_user_id, qb.created_at
        FROM question_banks qb
        WHERE (
            qb.owner_type = 'platform'
            OR EXISTS (
                SELECT 1 FROM workspace_members wm
                WHERE wm.workspace_id = qb.owner_workspace_id
                  AND wm.user_id = $1
                  AND wm.status = 'active'
            )
        )
        AND ($2::UUID IS NULL OR qb.owner_workspace_id = $2)
        AND ($3::TEXT IS NULL OR qb.subject = $3)
        AND ($4::TEXT IS NULL OR qb.exam_type = $4)
        AND qb.is_published = TRUE
        ORDER BY qb.subject ASC, qb.name ASC
        "#,
        auth.user_id,
        q.workspace_id,
        q.subject,
        exam_type_str
    )
    .fetch_all(&state.db)
    .await?;

    let banks = rows.into_iter().map(|r| {
        let exam_type: ExamType = serde_json::from_value(serde_json::json!(r.exam_type)).unwrap();
        QuestionBankResponse {
            id: r.id,
            owner_type: r.owner_type,
            name: r.name,
            description: r.description,
            subject: r.subject,
            exam_type,
            language: r.language,
            is_published: r.is_published,
            question_count: r.question_count,
            created_by_user_id: r.created_by_user_id,
            created_at: r.created_at,
        }
    }).collect();

    Ok(Json(banks))
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateBankRequest {
    #[validate(length(min = 2, max = 200))]
    pub name: String,

    pub description: Option<String>,

    #[validate(length(min = 1, max = 100))]
    pub subject: String,

    pub exam_type: ExamType,

    #[serde(default = "default_language")]
    pub language: String,
}

fn default_language() -> String { "en".to_string() }

pub async fn create_workspace_bank(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Json(req): Json<CreateBankRequest>,
) -> AppResult<(StatusCode, Json<QuestionBankResponse>)> {
    req.validate()?;

    if !ctx.role.is_tutor_or_above() {
        return Err(AppError::Forbidden("tutor role required".into()));
    }

    let bank_id = Uuid::now_v7();
    let exam_type_str = serde_json::to_value(&req.exam_type)?.as_str().unwrap().to_string();

    let row = sqlx::query!(
        r#"
        INSERT INTO question_banks (
            id, owner_type, owner_workspace_id, name, description,
            subject, exam_type, language, created_by_user_id
        )
        VALUES ($1, 'workspace', $2, $3, $4, $5, $6, $7, $8)
        RETURNING id, owner_type, owner_workspace_id, name, description,
                  subject, exam_type, language, is_published, question_count,
                  created_by_user_id, created_at
        "#,
        bank_id,
        ctx.workspace_id,
        req.name,
        req.description,
        req.subject,
        exam_type_str,
        req.language,
        ctx.user_id
    )
    .fetch_one(&state.db)
    .await?;

    let exam_type: ExamType = serde_json::from_value(serde_json::json!(row.exam_type)).unwrap();

    Ok((StatusCode::CREATED, Json(QuestionBankResponse { 
        id: row.id, 
        owner_type: row.owner_type, 
        name: row.name, 
        description: row.description, 
        subject: row.subject, 
        exam_type, 
        language: row.language, 
        is_published: row.is_published, 
        question_count: row.question_count, 
        created_by_user_id: row.created_by_user_id, 
        created_at: row.created_at 
    })))
}

pub async fn publish_bank(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(bank_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    let bank = sqlx::query!(
        r#"
        SELECT qb.owner_type, qb.owner_workspace_id, qb.created_by_user_id,
               qb.question_count, wm.role as "role?"
        FROM question_banks qb
        LEFT JOIN workspace_members wm
            ON wm.workspace_id = qb.owner_workspace_id AND wm.user_id = $2
        WHERE qb.id = $1
        "#,
        bank_id,
        auth.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("bank not found".into()))?;

    // Platform banks only edited by platform admin
    if bank.owner_type == "platform" {
        if !auth.is_platform_admin {
            return Err(AppError::Forbidden("platform admin only".into()));
        }
    } else {
        let role = bank.role.ok_or_else(|| AppError::NotFound("bank not found".into()));
        let is_creator = bank.created_by_user_id == auth.user_id;
        let is_admin = matches!(role.unwrap().as_str(), "proprietor" | "admin");
        if !is_creator && !is_admin {
            return Err(AppError::Forbidden("creator or admin only".into()));
        }
    }

    if bank.question_count == 0 {
        return Err(AppError::BadRequest("cannot publish emptybank".into()));
    }

    sqlx::query!(
        "UPDATE question_banks SET is_published = TRUE WHERE id = $1",
        bank_id
    )
    .execute(&state.db)
    .await?;

    Ok(StatusCode::NO_CONTENT)

}