use axum::{Json, extract::{Query, State}};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

use crate::{error::AppResult, state::AppState, workspaces::extractor::WorkspaceContext};

#[derive(Debug, Deserialize, Validate)]
pub struct SearchQuery {
    #[validate(length(min = 2, max = 200))]
    pub q: String,

    #[serde(default = "default_search_limit")]
    pub limit: u32 
}
fn default_search_limit() -> u32 {20}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub message_id: Uuid,
    pub content: String,
    pub snippet: String,
    pub channel_id: Uuid,
    pub channel_name: String,
    pub sender_user_id: Uuid,
    pub sender_full_name: String,
    pub sender_avatar_key: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct QueryResponse{
    pub results: Vec<SearchResult>,
}

pub async fn search(
    State(state): State<AppState>,
    ctx: WorkspaceContext,
    Query(query): Query<SearchQuery>
) -> AppResult<Json<QueryResponse>> {
    query.validate()?;

    let limit = query.limit.min(50) as i64;

    let rows = sqlx::query!(
        r#"
        SELECT
        m.id as message_id,
        m.content,
        m.channel_id as "channel_id!",
        m.sender_user_id,
        m.created_at,
        c.name as channel_name,
        u.full_name as sender_full_name,
        u.avatar_key as sender_avatar_key,
        ts_headline('english', m.content, plainto_tsquery('english', $2),
                    'StartSel=<mark>, StopSel=</mark>, MaxWords=20, MinWords=5')
            as "snippet!"
    FROM messages m
    JOIN channels c ON c.id = m.channel_id
    JOIN users u ON u.id = m.sender_user_id
    WHERE m.workspace_id = $1
      AND m.deleted = FALSE
      AND m.tsv @@ plainto_tsquery('english', $2)
      AND (
        c.visibility = 'public'
        OR EXISTS (
          SELECT 1 FROM channel_members
          WHERE channel_id = c.id AND user_id = $3
        )
      )
    ORDER BY ts_rank(m.tsv, plainto_tsquery('english', $2)) DESC
    LIMIT $4
        "#,
        ctx.workspace_id,
        query.q,
        ctx.user_id,
        limit
    )
    .fetch_all(&state.db)
    .await?;

    let results = rows.into_iter()
        .map(|row| SearchResult {
            message_id: row.message_id,
            content: row.content,
            snippet: row.snippet,
            channel_id: row.channel_id,
            channel_name: row.channel_name,
            sender_user_id: row.sender_user_id,
            sender_full_name: row.sender_full_name,
            sender_avatar_key: row.sender_avatar_key,
            created_at: row.created_at,
        })
        .collect();

    Ok(Json(QueryResponse { results }))
}