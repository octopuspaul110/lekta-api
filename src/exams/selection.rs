use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionCriteria {
    pub sources: Vec<SelectionSource>
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionSource {
    pub bank_id: Uuid,
    pub count: i32,
    #[serde(default)]
    pub filters: SelectionFilters,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SelectionFilters {
    pub topic_tags: Option<Vec<String>>,
    pub difficulty: Option<Vec<String>>,
    pub year_min: Option<i32>,
    pub year_max: Option<i32>,
}

pub struct ResolvedQuestion {
    pub id: Uuid,
    pub question_text: String,
    pub question_type: String,
    pub options: Option<serde_json::Value>,
    pub correct_answer: serde_json::Value,
    pub marks: i32,
}

// Valaidate that criteria can produce the requested number of questions
// Returns the toal marks if valid
pub async fn validate_criteria(
    db: &PgPool,
    criteria: &SelectionCriteria,
    workspace_id: Uuid
) -> AppResult<i64> {
    if criteria.sources.is_empty() {
        return Err(AppError::BadRequest("selection_criteria must have at least one source".into()));
    }

    let mut total_marks: i64 = 0;

    for source in &criteria.sources {
        if source.count <= 0 {
            return Err(AppError::BadRequest("source count must be positive".into()));
        }

        // Verifybank is accessible (platform or owned by workspace)
        let bank = sqlx::query!(
            r#"
            SELECT owner_type, owner_workspace_id, is_published
            FROM question_banks WHERE id = $1
            "#,
            source.bank_id
        )
        .fetch_optional(db)
        .await?
        .ok_or_else(|| AppError::BadRequest(format!("bank {} not found",source.bank_id)))?;

        if !bank.is_published{
            return Err(AppError::Forbidden("cannot use unpublished bank".into()));
        }

        if bank.owner_type == "workspace" && bank.owner_workspace_id != Some(workspace_id){
            return Err(AppError::Forbidden("bank not accessible in this workspace".into()))
        }

        // Count available questions matching filters
        let available = sqlx::query!(
            r#"
            SELECT COUNT(*) as "count!", COALESCE(SUM(marks), 0) as "sum_marks!"
            FROM questions
            WHERE question_bank_id = $1
              AND deleted_at IS NULL
              AND ($2::TEXT[] IS NULL OR topic_tags && $2)
              AND ($3::TEXT[] IS NULL OR difficulty = ANY($3))
              AND ($4::INTEGER IS NULL OR year >= $4)
              AND ($5::INTEGER IS NULL OR year <= $5)
            "#,
            source.bank_id,
            source.filters.topic_tags.as_deref(),
            source.filters.difficulty.as_deref(),
            source.filters.year_min,
            source.filters.year_max
        )
        .fetch_one(db)
        .await?;
        
        if available.count < source.count as i64 {
            return Err(AppError::UnprocessableEntity(format!("source {} needs {} questions but only {} available", source.bank_id, source.count, available.count)));
        }

        //  Approximate marks — this is an overestimate but close enough for total_marks
        // In materialization we compute the real total from selected questions
        let avg_marks = available.sum_marks as f64 / available.count as f64;
        total_marks += (avg_marks * source.count as f64) as i64;
    }
    Ok(total_marks)
}


pub async fn resolve_questions(
    db: &PgPool,
    criteria: &SelectionCriteria,
) -> AppResult<Vec<ResolvedQuestion>> {
    let mut all_questions = Vec::new();

    for source in &criteria.sources {
        let rows = sqlx::query!(
            r#"
            SELECT id, question_text, question_type, options, correct_answers, marks
            FROM questions
            WHERE question_bank_id = $1
              AND deleted_at IS NULL
              AND ($2::TEXT[] IS NULL OR topic_tags && $2)
              AND ($3::TEXT[] IS NULL OR difficulty = ANY($3))
              AND ($4::INTEGER IS NULL OR year >= $4)
              AND ($5::INTEGER IS NULL OR year <= $5)
            ORDER BY RANDOM()
            LIMIT $6
            "#,
            source.bank_id,
            source.filters.topic_tags.as_deref(),
            source.filters.difficulty.as_deref(),
            source.filters.year_min,
            source.filters.year_max,
            source.count as i64
        )
        .fetch_all(db)
        .await?;

    for row in rows {
            all_questions.push(ResolvedQuestion {
                id: row.id,
                question_text: row.question_text,
                question_type: row.question_type,
                options: row.options,
                correct_answer: row.correct_answers,
                marks: row.marks,
            });
        }
    }
    Ok(all_questions)
}