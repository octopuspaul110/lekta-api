use axum::{Json, extract::{Path, State}};
use chrono::{DateTime, Utc};
use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{auth::extractor::AuthUser, error::{AppError, AppResult}, exams::types::{Difficulty, QuestionType}, state::{AppState}};

#[derive(Debug, Serialize)]
pub struct QuestionResponse {
    pub id: Uuid,
    pub question_bank_id: Uuid,
    pub question_text: String,
    pub question_type: QuestionType,
    pub options: Option<serde_json::Value>,
    pub correct_answer: serde_json::Value,
    pub explanation: Option<String>,
    pub difficulty: Difficulty,
    pub topic_tags: Vec<String>,
    pub year: Option<i32>,
    pub marks: i32,
    pub media_s3_keys: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct QuestionInput {
    pub question_text: String,
    pub question_type: QuestionType,
    pub options: Option<serde_json::Value>,
    pub correct_answer: serde_json::Value,
    pub explanation: Option<String>,
    pub difficulty: Difficulty,
    
    #[serde(default)]
    pub topic_tags: Vec<String>,

    pub year: Option<i32>,

    #[serde(default = "default_marks")]
    #[validate(range(min = 1))]
    pub marks: i32,

    #[serde(default = "default_media")]
    pub media_s3_keys: serde_json::Value,
}

fn default_marks() -> i32 { 1 }
fn default_media() -> serde_json::Value { serde_json::json!([])}

#[derive(Debug, Deserialize)]
pub struct CreateQuestionsRequest {
    pub questions: Vec<QuestionInput>,
}

pub async fn create_questions(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(bank_id): Path<Uuid>,
    Json(req): Json<CreateQuestionsRequest>,
) -> AppResult<(StatusCode, Json<Vec<QuestionResponse>>)> {
    if req.questions.is_empty() {
        return Err(AppError::BadRequest("no questions provided".into()));
    }

    // Validate each
    for q in &req.questions {
        q.validate()?;
    }

    // Check bank ownership
    let bank = sqlx::query!(
        r#"
        SELECT qb.owner_type, qb.owner_workspace_id, qb.created_by_user_id,
               wm.role as "role?"
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

    if bank.owner_type == "platform" {
        if !auth.is_platform_admin {
            return  Err(AppError::Forbidden("platform admin only".into()));
        }
    } else {
        let role = bank.role.ok_or_else(|| AppError::NotFound("bank not found".into()))?;
        let is_creator = bank.created_by_user_id == auth.user_id;
        let is_tutor_or_above = matches!(role.as_str(), "proprietor" | "admin" | "tutor");
        if !is_creator && !is_tutor_or_above {
            return Err(AppError::Forbidden("creator or workspace tutor only".into()));
        }
    }

    let mut tx = state.db.begin().await?;
    let mut inserted = Vec::new();

    for q in &req.questions {
        let question_id = Uuid::now_v7();
        let question_type_str = serde_json::to_value(&q.question_text)?.as_str().unwrap().to_string();
        let difficulty_str = serde_json::to_value(&q.difficulty)?.as_str().unwrap().to_string();

        let row = sqlx::query!(
            r#"
            INSERT INTO questions (
                id, question_bank_id, question_text, question_type,
                options, correct_answers , explanation, difficulty,
                topic_tags, year, marks, media_keys 
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            RETURNING id, question_bank_id, question_text, question_type,
                      options, correct_answers as "correct_answer", explanation, difficulty, topic_tags, year, marks, media_keys as "media_s3_keys", created_at
            "#,
            question_id,
            bank_id,
            q.question_text,
            question_type_str,
            q.options,
            q.correct_answer,
            q.explanation,
            difficulty_str,
            &q.topic_tags,
            q.year,
            q.marks,
            q.media_s3_keys
        )
        .fetch_one(&state.db)
        .await?;

        let question_type: QuestionType = serde_json::from_value(serde_json::json!(row.question_type)).unwrap();
        let difficulty: Difficulty = serde_json::from_value(serde_json::json!(row.difficulty)).unwrap();

        inserted.push(QuestionResponse {
            id: row.id,
            question_bank_id: row.question_bank_id,
            question_text: row.question_text,
            question_type,
            options: row.options,
            correct_answer: row.correct_answer,
            explanation: row.explanation,
            difficulty,
            topic_tags: row.topic_tags,
            year: row.year,
            marks: row.marks,
            media_s3_keys: row.media_s3_keys,
            created_at: row.created_at,
        });
    }

    // Update the bank's question count
    sqlx::query!(
        "UPDATE question_banks SET question_count = question_count + $1 WHERE id = $2",
        req.questions.len() as i32,
        &bank_id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok((StatusCode::CREATED, Json(inserted)))
}

pub async fn list_questions(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(bank_id): Path<Uuid>,
) -> AppResult<Json<Vec<QuestionResponse>>> {
    // Verify access to bank
    let bank = sqlx::query!(
        r#"
        SELECT qb.owner_type, qb.owner_workspace_id, qb.is_published,
               wm.role as "role?"
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

    // Platform bank: angone can see published; only platform admin can see unpublished
    // Workspace bank: any workspace member can see published; only creator or admin can see unpublished
    let can_access = if bank.owner_type == "platform" {
        bank.is_published || auth.is_platform_admin
    } else {
        bank.role.is_some()
    };

    if !can_access {
        return Err(AppError::NotFound("bank not found".into()));
    }

    let row = sqlx::query!(
        r#"
        SELECT id, question_bank_id, question_text, question_type, options,
               correct_answers as "correct_answer", explanation, difficulty, topic_tags, year, marks,
               media_keys as "media_s3_keys", created_at
        FROM questions
        WHERE question_bank_id = $1 AND deleted_at IS NULL
        ORDER BY created_at ASC
        "#,
        bank_id
    )
    .fetch_all(&state.db)
    .await?;

    let questions = row.into_iter().map(|r| {
        let question_type: QuestionType = serde_json::from_value(serde_json::json!(r.question_type)).unwrap();
        let difficulty: Difficulty = serde_json::from_value(serde_json::json!(r.difficulty)).unwrap();
        QuestionResponse {
            id: r.id,
            question_bank_id: r.question_bank_id,
            question_text: r.question_text,
            question_type,
            options: r.options,
            correct_answer: r.correct_answer,
            explanation: r.explanation,
            difficulty,
            topic_tags: r.topic_tags,
            year: r.year,
            marks: r.marks,
            media_s3_keys: r.media_s3_keys,
            created_at: r.created_at,
        }
    }).collect();

    Ok(Json(questions))
}
pub async fn delete_question(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(question_id): Path<Uuid>,
) -> AppResult<StatusCode> {
    let question = sqlx::query!(
        r#"
        SELECT q.question_bank_id, qb.owner_type, qb.owner_workspace_id,
               qb.created_by_user_id, wm.role as "role?"
        FROM questions q
        JOIN question_banks qb ON qb.id = q.question_bank_id
        LEFT JOIN workspace_members wm
            ON wm.workspace_id = qb.owner_workspace_id AND wm.user_id = $2
        WHERE q.id = $1 AND q.deleted_at IS NULL
        "#,
        question_id,
        auth.user_id
    )
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::Forbidden("platform admin only".into()))?;

    if question.owner_type == "platform" {
        if !auth.is_platform_admin {
            return Err(AppError::Forbidden("platform admin only".into()));
        }
    } else {
        let role = question.role.ok_or_else(|| AppError::NotFound("question not found".into()))?;
        let is_creator = question.created_by_user_id == auth.user_id;
        let is_admin = matches!(role.as_str(), "proprietor" | "admin");
        if !is_creator && !is_admin {
            return Err(AppError::Forbidden("creator or admin only".into()));
        }
    }

    let mut tx = state.db.begin().await?;

    sqlx::query!(
        "UPDATE questions SET deleted_at = NOW() WHERE id = $1",
        question_id
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query!(
        "UPDATE question_banks SET question_count = question_count - 1 WHERE id = $1",
        question.question_bank_id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(StatusCode::NO_CONTENT)
}