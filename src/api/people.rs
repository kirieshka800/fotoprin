//! Глобальный поиск по задачам и встречам (строка поиска в шапке).

use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::*;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    q: Option<String>,
}

pub async fn search(
    State(state): State<AppState>,
    Extension(auth): Extension<CurrentUser>,
    Query(query): Query<SearchQuery>,
) -> ApiResult<Json<Value>> {
    current_user(&state.db, &auth).await?;
    let text = query.q.unwrap_or_default();
    let text = text.trim();
    if text.is_empty() {
        return Ok(Json(json!([])));
    }
    let pattern = like_pattern(text);
    let meetings: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, title FROM meetings WHERE title ILIKE $1 ORDER BY id DESC LIMIT 5")
            .bind(&pattern)
            .fetch_all(&state.db)
            .await?;
    let tasks: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, title FROM tasks WHERE title ILIKE $1 ORDER BY id DESC LIMIT 5")
            .bind(&pattern)
            .fetch_all(&state.db)
            .await?;
    let mut found: Vec<Value> = Vec::new();
    for (id, title) in meetings {
        found.push(json!({"type": "meeting", "id": id, "title": title}));
    }
    for (id, title) in tasks {
        found.push(json!({"type": "task", "id": id, "title": title}));
    }
    found.truncate(8);
    Ok(Json(Value::Array(found)))
}
