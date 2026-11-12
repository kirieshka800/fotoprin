//! Уведомления: хранение, выдача ленты, отметки «прочитано» и вспомогательные функции,
//! через которые другие модули (задачи, график, напоминания) создают уведомления.

use axum::{
    extract::{Path, Query, State},
        Json,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use super::*;
use crate::AppState;

#[derive(Debug, Clone)]
pub struct NewNotice {
    pub title: String,
    pub text: String,
    pub category: String,
    pub action_label: String,
    pub href: String,
    pub from_ai: bool,
    pub kind: Option<String>,
    pub ref_key: Option<String>,
}

impl NewNotice {
    pub fn new(
        title: impl Into<String>,
        text: impl Into<String>,
        category: &str,
        action_label: &str,
        href: impl Into<String>,
        from_ai: bool,
    ) -> Self {
        NewNotice {
            title: title.into(),
            text: text.into(),
            category: category.to_string(),
            action_label: action_label.to_string(),
            href: href.into(),
            from_ai,
            kind: None,
            ref_key: None,
        }
    }

    pub fn with_ref(mut self, kind: &str, ref_key: &str) -> Self {
        self.kind = Some(kind.to_string());
        self.ref_key = Some(ref_key.to_string());
        self
    }
}

pub async fn notify_one(db: &PgPool, recipient_id: i64, n: &NewNotice) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO notifications
            (recipient_id, title, text, category, action_label, href, from_ai, kind, ref_key)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(recipient_id)
    .bind(&n.title)
    .bind(&n.text)
    .bind(&n.category)
    .bind(&n.action_label)
    .bind(&n.href)
    .bind(n.from_ai)
    .bind(&n.kind)
    .bind(&n.ref_key)
    .execute(db)
    .await?;
    Ok(())
}

/// Уведомление всем руководителям (кроме `except` — того, кто сам совершил действие).
pub async fn notify_managers(db: &PgPool, except: Option<i64>, n: &NewNotice) -> Result<(), sqlx::Error> {
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM employees WHERE role = 'Руководитель' AND ($1::bigint IS NULL OR id <> $1)",
    )
    .bind(except)
    .fetch_all(db)
    .await?;
    for id in ids {
        notify_one(db, id, n).await?;
    }
    Ok(())
}

/// Побочные уведомления не должны ломать основной запрос: ошибка только логируется.
pub fn log_notify_error(result: Result<(), sqlx::Error>) {
    if let Err(err) = result {
        eprintln!("Не удалось создать уведомление: {}", err);
    }
}

/// Помечает прочитанными уведомления определённого вида (например, когда предложение графика решено).
pub async fn mark_kind_read(db: &PgPool, kind: &str, ref_key: &str) {
    let result = sqlx::query(
        "UPDATE notifications SET is_read = TRUE WHERE kind = $1 AND ref_key = $2 AND is_read = FALSE",
    )
    .bind(kind)
    .bind(ref_key)
    .execute(db)
    .await;
    if let Err(err) = result {
        eprintln!("Не удалось пометить уведомления прочитанными: {}", err);
    }
}

// ---------------------------------------------------------------------------
// Лента
// ---------------------------------------------------------------------------
#[derive(Debug, Deserialize)]
pub struct ListQuery {
    filter: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct NoticeRow {
    id: i64,
    title: String,
    text: String,
    category: String,
    action_label: String,
    href: String,
    is_read: bool,
    from_ai: bool,
    created_at: DateTime<Utc>,
}

pub async fn list_notifications(
    State(state): State<AppState>,
    Extension(auth): Extension<CurrentUser>,
    Query(query): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    let user = current_user(&state.db, &auth).await?;
    let filter = match query.filter.as_deref() {
        Some("unread") => "unread",
        Some("ai") => "ai",
        _ => "all",
    };

    let rows = sqlx::query_as::<_, NoticeRow>(
        "SELECT id, title, text, category, action_label, href, is_read, from_ai, created_at
         FROM notifications
         WHERE recipient_id = $1
           AND ($2::text = 'all'
                OR ($2::text = 'unread' AND is_read = FALSE)
                OR ($2::text = 'ai' AND from_ai = TRUE))
         ORDER BY created_at DESC, id DESC
         LIMIT 200",
    )
    .bind(user.id)
    .bind(filter)
    .fetch_all(&state.db)
    .await?;

    let counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT COUNT(*),
                COUNT(*) FILTER (WHERE is_read = FALSE),
                COUNT(*) FILTER (WHERE from_ai = TRUE)
         FROM notifications WHERE recipient_id = $1",
    )
    .bind(user.id)
    .fetch_one(&state.db)
    .await?;

    let items: Vec<Value> = rows
        .iter()
        .map(|n| {
            json!({
                "id": n.id,
                "at": n.created_at,
                "title": n.title,
                "text": n.text,
                "category": n.category,
                "actionLabel": n.action_label,
                "href": n.href,
                "read": n.is_read,
                "fromAi": n.from_ai,
            })
        })
        .collect();

    // Блоки справа показываем руководителю: решение по графику и ближайшее напоминание.
    let offset = app_offset(&state.db).await;
    let pending = if user.is_manager() {
        schedule::pending_summary(&state.db).await?
    } else {
        None
    };
    let next_reminder = reminders::next_reminder(&state.db, &user, &offset).await?;

    Ok(Json(json!({
        "items": items,
        "counts": { "all": counts.0, "unread": counts.1, "ai": counts.2 },
        "pending": pending,
        "nextReminder": next_reminder,
    })))
}

pub async fn read_one(
    State(state): State<AppState>,
    Extension(auth): Extension<CurrentUser>,
    Path(id_raw): Path<String>,
) -> ApiResult<Json<Value>> {
    let user = current_user(&state.db, &auth).await?;
    let id = parse_id(&id_raw, "Уведомление")?;
    let result = sqlx::query("UPDATE notifications SET is_read = TRUE WHERE id = $1 AND recipient_id = $2")
        .bind(id)
        .bind(user.id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found("Уведомление не найдено"));
    }
    Ok(Json(json!({ "ok": true })))
}

pub async fn read_all(State(state): State<AppState>, Extension(auth): Extension<CurrentUser>) -> ApiResult<Json<Value>> {
    let user = current_user(&state.db, &auth).await?;
    let result = sqlx::query("UPDATE notifications SET is_read = TRUE WHERE recipient_id = $1 AND is_read = FALSE")
        .bind(user.id)
        .execute(&state.db)
        .await?;
    Ok(Json(json!({ "ok": true, "updated": result.rows_affected() })))
}

/// Notifications are private to the current employee, regardless of role.
pub async fn delete_one(State(state):State<AppState>,Extension(auth):Extension<CurrentUser>,Path(raw):Path<String>)->ApiResult<Json<Value>> {
    let user=current_user(&state.db,&auth).await?;let id=parse_id(&raw,"Уведомление")?;
    let n=sqlx::query("DELETE FROM notifications WHERE id=$1 AND recipient_id=$2").bind(id).bind(user.id).execute(&state.db).await?.rows_affected();
    if n==0 {return Err(ApiError::not_found("Уведомление не найдено"));}
    Ok(Json(json!({"ok":true})))
}
pub async fn delete_all(State(state):State<AppState>,Extension(auth):Extension<CurrentUser>)->ApiResult<Json<Value>> {
    let user=current_user(&state.db,&auth).await?;
    let n=sqlx::query("DELETE FROM notifications WHERE recipient_id=$1").bind(user.id).execute(&state.db).await?.rows_affected();
    Ok(Json(json!({"ok":true,"deleted":n})))
}
