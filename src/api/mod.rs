//! REST API разделов «Задачи», «График работы» и «Уведомления»
//! (+ общие справочники: пользователь, сотрудники, настройки, поиск).
//! Контракт запросов/ответов описан в static/api.js.

use axum::{
    routing::{get, post, put},
    Extension, Router,
};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;

use crate::auth::CurrentUser;
use crate::error::{ApiError, ApiResult};
use crate::{util, AppState};

pub mod notifications;
pub mod people;
pub mod reminders;
pub mod schedule;
pub mod tasks;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/search", get(people::search))
        // задачи
        .route("/api/tasks", get(tasks::list_tasks).post(tasks::create_task))
        .route(
            "/api/tasks/:id",
            get(tasks::get_task)
                .patch(tasks::update_task)
                .delete(tasks::delete_task),
        )
        .route("/api/tasks/:id/comments", post(tasks::add_comment))
        .route("/api/tasks/:id/reminder", put(tasks::set_reminder))
        .route("/api/reminders", get(reminders::list_reminders))
        // график работы
        .route("/api/schedule", get(schedule::get_schedule))
        .route("/api/schedule/shifts", post(schedule::add_shift))
        .route(
            "/api/schedule/shifts/:id",
            put(schedule::update_shift).delete(schedule::delete_shift),
        )
        .route("/api/schedule/generate", post(schedule::generate))
        .route("/api/schedule/proposal/reject", post(schedule::reject_proposal))
        .route("/api/schedule/publish", post(schedule::publish))
        // уведомления
        .route("/api/notifications", get(notifications::list_notifications))
        .route("/api/notifications/read-all", post(notifications::read_all))
        .route("/api/notifications/:id/read", post(notifications::read_one))
}

// ---------------------------------------------------------------------------
// Сотрудник / текущий пользователь
// ---------------------------------------------------------------------------
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Employee {
    pub id: i64,
    pub name: String,
    pub position: String,
    pub role: String,
    pub department: String,
    pub email: Option<String>,
}

pub const EMPLOYEE_COLS: &str = "id, name, position, role, department, email";

impl Employee {
    pub fn is_manager(&self) -> bool {
        self.role == "Руководитель"
    }

    pub fn initials(&self) -> String {
        let letters: String = self
            .name
            .split_whitespace()
            .filter_map(|w| w.chars().next())
            .take(2)
            .collect();
        letters.to_uppercase()
    }

    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "position": self.position,
            "department": self.department,
            "role": self.role,
            "email": self.email,
        })
    }
}

/// Сотрудник, соответствующий вошедшему пользователю (таблица users из auth.rs).
/// Ищем по почте, затем по имени; если такого сотрудника ещё нет — создаём, чтобы пользователь
/// появился в списках ответственных и мог получать уведомления. Администратор всегда руководитель.
pub async fn current_user(db: &PgPool, auth: &CurrentUser) -> ApiResult<Employee> {
    let found = sqlx::query_as::<_, Employee>(&format!(
        "SELECT {} FROM employees
         WHERE LOWER(email) = LOWER($1) OR LOWER(name) = LOWER($2)
         ORDER BY (LOWER(email) = LOWER($1)) DESC, id LIMIT 1",
        EMPLOYEE_COLS
    ))
    .bind(&auth.email)
    .bind(&auth.name)
    .fetch_optional(db)
    .await?;

    let mut employee = match found {
        Some(emp) => {
            if emp.email.is_none() {
                let _ = sqlx::query("UPDATE employees SET email = LOWER($2) WHERE id = $1 AND email IS NULL")
                    .bind(emp.id)
                    .bind(&auth.email)
                    .execute(db)
                    .await;
            }
            emp
        }
        None => {
            let role = if auth.role == "admin" || auth.role == "manager" {
                "Руководитель"
            } else {
                "Сотрудник"
            };
            sqlx::query_as::<_, Employee>(&format!(
                "INSERT INTO employees (name, position, role, department, email)
                 VALUES ($1, $2, $3, '', LOWER($4)) RETURNING {}",
                EMPLOYEE_COLS
            ))
            .bind(&auth.name)
            .bind(&auth.position)
            .bind(role)
            .bind(&auth.email)
            .fetch_one(db)
            .await?
        }
    };
    if auth.role == "admin" {
        employee.role = "Руководитель".to_string();
    }
    Ok(employee)
}

pub fn require_manager(user: &Employee) -> ApiResult<()> {
    if user.is_manager() {
        Ok(())
    } else {
        Err(ApiError::forbidden("Действие доступно только руководителю"))
    }
}

pub fn parse_id(raw: &str, what: &str) -> ApiResult<i64> {
    raw.trim()
        .parse::<i64>()
        .map_err(|_| ApiError::not_found(format!("{} не найден(а)", what)))
}

/// id в запросах бывает строкой ("12") или числом (12).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum FlexId {
    Num(i64),
    Text(String),
}

impl FlexId {
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            FlexId::Num(n) => Some(*n),
            FlexId::Text(s) => s.trim().parse().ok(),
        }
    }
    pub fn is_blank(&self) -> bool {
        matches!(self, FlexId::Text(s) if s.trim().is_empty())
    }
    pub fn to_text(&self) -> String {
        match self {
            FlexId::Num(n) => n.to_string(),
            FlexId::Text(s) => s.trim().to_string(),
        }
    }
}

pub async fn employee_exists(db: &PgPool, id: i64) -> Result<bool, sqlx::Error> {
    let found: Option<(i64,)> = sqlx::query_as("SELECT id FROM employees WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await?;
    Ok(found.is_some())
}

// ---------------------------------------------------------------------------
// Настройки и часовой пояс
// ---------------------------------------------------------------------------
pub fn default_settings() -> Value {
    json!({
        "company": "Фотопринт",
        "timezone": "Москва · UTC+3",
        "autonomous": true,
        "createTasks": true,
        "sendReminders": true,
        "publishRule": "Только после подтверждения",
        "telegram": {"connected": false, "enabled": false},
        "email": {"connected": false, "enabled": false, "address": ""},
        "quietHours": {"enabled": true, "from": "20:00", "to": "09:00"}
    })
}

pub fn merge_json(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(k) {
                    Some(existing) if existing.is_object() && v.is_object() => merge_json(existing, v),
                    _ => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (b, o) => *b = o.clone(),
    }
}

/// Настройки из БД поверх значений по умолчанию. Не падает: при ошибке вернёт значения по умолчанию.
pub async fn load_settings(db: &PgPool) -> Value {
    let mut settings = default_settings();
    let stored: Result<Option<Value>, _> =
        sqlx::query_scalar("SELECT data FROM app_settings WHERE id = 1")
            .fetch_optional(db)
            .await;
    if let Ok(Some(data)) = stored {
        merge_json(&mut settings, &data);
    }
    settings
}

pub fn settings_offset(settings: &Value) -> FixedOffset {
    let secs = settings["timezone"]
        .as_str()
        .and_then(util::parse_utc_offset)
        .unwrap_or(3 * 3600);
    FixedOffset::east_opt(secs).unwrap_or_else(|| FixedOffset::east_opt(3 * 3600).unwrap())
}

pub async fn app_offset(db: &PgPool) -> FixedOffset {
    settings_offset(&load_settings(db).await)
}

// ---------------------------------------------------------------------------
// Даты
// ---------------------------------------------------------------------------
pub fn local_to_utc(offset: &FixedOffset, naive: NaiveDateTime) -> DateTime<Utc> {
    offset
        .from_local_datetime(&naive)
        .single()
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|| Utc::now() + chrono::Duration::days(1))
}

/// Принимает RFC 3339 ("2026-11-12T13:00:00.000Z") и «локальные» строки без пояса
/// ("2026-11-12T16:00", "2026-11-12 16:00", "2026-11-12"), которые трактуются во времени компании.
pub fn parse_due(text: &str, offset: &FixedOffset) -> Option<DateTime<Utc>> {
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(t) {
        return Some(dt.with_timezone(&Utc));
    }
    for fmt in ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(t, fmt) {
            return Some(local_to_utc(offset, naive));
        }
    }
    if let Ok(date) = NaiveDate::parse_from_str(t, "%Y-%m-%d") {
        if let Some(naive) = date.and_hms_opt(12, 0, 0) {
            return Some(local_to_utc(offset, naive));
        }
    }
    None
}

/// «12 ноября, 18:00» во времени компании.
pub fn fmt_local(dt: DateTime<Utc>, offset: &FixedOffset) -> String {
    use chrono::{Datelike, Timelike};
    let local = dt.with_timezone(offset);
    format!(
        "{} {}, {:02}:{:02}",
        local.day(),
        util::MONTHS_FULL[local.month0() as usize],
        local.hour(),
        local.minute()
    )
}

/// Экранирование для ILIKE.
pub fn like_pattern(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{}%", escaped)
}
