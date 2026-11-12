//! Задачи: список, карточка, создание, редактирование, комментарии, напоминание.
//! Также создание задач из протокола встречи, подготовленного нейросетью.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, Datelike, Duration, FixedOffset, TimeZone, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::collections::HashMap;

use super::notifications::{log_notify_error, notify_managers, notify_one, NewNotice};
use super::*;
use crate::AppState;

const STATUSES: [&str; 4] = ["planned", "in_progress", "review", "done"];
const PRIORITIES: [&str; 3] = ["Обычный", "Высокий", "Срочный"];
const CHANNELS: [&str; 3] = ["Telegram и почта", "Только Telegram", "Только почта"];

fn status_label(status: &str) -> &'static str {
    match status {
        "in_progress" => "В работе",
        "review" => "На проверке",
        "done" => "Завершено",
        _ => "Запланировано",
    }
}

fn before_info(code: &str) -> Option<(i32, &'static str)> {
    match code {
        "15m" => Some((15, "За 15 минут до срока")),
        "1h" => Some((60, "За час до срока")),
        "1d" => Some((1440, "За день до срока")),
        "0" => Some((0, "В момент срока")),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Чтение
// ---------------------------------------------------------------------------
#[derive(Debug, sqlx::FromRow)]
struct TaskRow {
    id: i64,
    title: String,
    description: String,
    assignee_id: Option<i64>,
    assignee_name: Option<String>,
    due_at: DateTime<Utc>,
    status: String,
    priority: String,
    source: Option<String>,
    meeting_id: Option<i64>,
    quote_title: Option<String>,
    quote_text: Option<String>,
    checklist: Value,
    created_at: DateTime<Utc>,
}

const TASK_SELECT: &str = "SELECT t.id, t.title, t.description, t.assignee_id, e.name AS assignee_name,
        t.due_at, t.status, t.priority, t.source, t.meeting_id, t.quote_title, t.quote_text,
        t.checklist, t.created_at
    FROM tasks t LEFT JOIN employees e ON e.id = t.assignee_id";

fn task_json(row: &TaskRow) -> Value {
    let quote = match (&row.quote_title, &row.quote_text) {
        (Some(title), Some(text)) => json!({ "title": title, "text": text }),
        _ => Value::Null,
    };
    json!({
        "id": row.id,
        "code": format!("FP-{}", 100 + row.id),
        "title": row.title,
        "assigneeId": row.assignee_id,
        "assigneeName": row.assignee_name.clone().unwrap_or_default(),
        "due": row.due_at,
        "status": row.status,
        "priority": row.priority,
        "source": row.source.clone().unwrap_or_else(|| "Вручную".to_string()),
        "meetingId": row.meeting_id,
        "description": row.description,
        "quote": quote,
        "checklist": row.checklist,
        "createdAt": row.created_at,
    })
}

async fn fetch_row(db: &PgPool, id: i64) -> ApiResult<TaskRow> {
    sqlx::query_as::<_, TaskRow>(&format!("{} WHERE t.id = $1", TASK_SELECT))
        .bind(id)
        .fetch_optional(db)
        .await?
        .ok_or_else(|| ApiError::not_found("Задача не найдена"))
}

/// Полная карточка: задача + комментарии + история + напоминание + связь с графиком.
async fn load_task(db: &PgPool, id: i64) -> ApiResult<Value> {
    let row = fetch_row(db, id).await?;
    let mut task = task_json(&row);

    let comments: Vec<(i64, String, bool, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, author_name, is_ai, text, created_at FROM task_comments
         WHERE task_id = $1 ORDER BY created_at, id",
    )
    .bind(id)
    .fetch_all(db)
    .await?;
    task["comments"] = Value::Array(
        comments
            .into_iter()
            .map(|(cid, author, is_ai, text, at)| {
                json!({ "id": cid, "authorName": author, "isAi": is_ai, "text": text, "at": at })
            })
            .collect(),
    );

    let history: Vec<(i64, String, DateTime<Utc>)> = sqlx::query_as(
        "SELECT id, text, created_at FROM task_history WHERE task_id = $1 ORDER BY created_at, id",
    )
    .bind(id)
    .fetch_all(db)
    .await?;
    task["history"] = Value::Array(
        history
            .into_iter()
            .map(|(hid, text, at)| json!({ "id": hid, "text": text, "at": at }))
            .collect(),
    );

    let reminder: Option<(DateTime<Utc>, String, String, String)> = sqlx::query_as(
        "SELECT remind_at, text, before_code, channel FROM reminders WHERE task_id = $1",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;
    task["reminder"] = match reminder {
        Some((at, text, before, channel)) => {
            json!({ "at": at, "text": text, "before": before, "channel": channel })
        }
        None => Value::Null,
    };

    task["scheduleLink"] = Value::Null;
    if row.title.to_lowercase().contains("график") && row.status != "done" {
        if let Some((people, shifts)) = schedule::pending_counts(db).await? {
            task["scheduleLink"] = json!({
                "text": format!(
                    "{} {} · {} {}. Изменения ещё не опубликованы.",
                    people,
                    util::plural(people, "сотрудник", "сотрудника", "сотрудников"),
                    shifts,
                    util::plural(shifts, "смена", "смены", "смен"),
                )
            });
        }
    }
    Ok(task)
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    status: Option<String>,
    #[serde(rename = "assigneeId")]
    assignee_id: Option<String>,
    sort: Option<String>,
    page: Option<i64>,
    #[serde(rename = "pageSize")]
    page_size: Option<i64>,
    #[serde(rename = "meetingId")]
    meeting_id: Option<String>,
}

pub async fn list_tasks(
    State(state): State<AppState>,
    Extension(auth): Extension<CurrentUser>,
    Query(query): Query<ListQuery>,
) -> ApiResult<Json<Value>> {
    current_user(&state.db, &auth).await?;

    let status = match query.status.as_deref() {
        Some("all") => "all",
        Some("done") => "done",
        _ => "active",
    };
    let assignee: Option<i64> = query
        .assignee_id
        .as_deref()
        .and_then(|s| s.trim().parse().ok());
    let meeting: Option<i64> = query
        .meeting_id
        .as_deref()
        .and_then(|s| s.trim().parse().ok());
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);
    let page = query.page.unwrap_or(1).max(1);

    let order = match query.sort.as_deref() {
        Some("title") => "t.title ASC, t.id",
        Some("status") => {
            "CASE t.status WHEN 'in_progress' THEN 0 WHEN 'review' THEN 1 WHEN 'planned' THEN 2 ELSE 3 END, t.due_at, t.id"
        }
        _ => "t.due_at ASC, t.id",
    };

    let sql = format!(
        "{} WHERE ($1::bigint IS NULL OR t.assignee_id = $1)
           AND ($2::bigint IS NULL OR t.meeting_id = $2)
           AND ($3::text = 'all'
                OR ($3::text = 'active' AND t.status <> 'done')
                OR ($3::text = 'done' AND t.status = 'done'))
         ORDER BY {} LIMIT $4 OFFSET $5",
        TASK_SELECT, order
    );
    let rows = sqlx::query_as::<_, TaskRow>(&sql)
        .bind(assignee)
        .bind(meeting)
        .bind(status)
        .bind(page_size)
        .bind((page - 1) * page_size)
        .fetch_all(&state.db)
        .await?;

    // Счётчики вкладок учитывают фильтры сотрудника и встречи, но не вкладку статуса.
    let counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT COUNT(*) FILTER (WHERE status <> 'done'),
                COUNT(*),
                COUNT(*) FILTER (WHERE status = 'done')
         FROM tasks
         WHERE ($1::bigint IS NULL OR assignee_id = $1)
           AND ($2::bigint IS NULL OR meeting_id = $2)",
    )
    .bind(assignee)
    .bind(meeting)
    .fetch_one(&state.db)
    .await?;
    let total = match status {
        "all" => counts.1,
        "done" => counts.2,
        _ => counts.0,
    };

    Ok(Json(json!({
        "items": rows.iter().map(task_json).collect::<Vec<Value>>(),
        "total": total,
        "counts": { "active": counts.0, "all": counts.1, "done": counts.2 },
    })))
}

pub async fn get_task(
    State(state): State<AppState>,
    Extension(auth): Extension<CurrentUser>,
    Path(id_raw): Path<String>,
) -> ApiResult<Json<Value>> {
    current_user(&state.db, &auth).await?;
    let id = parse_id(&id_raw, "Задача")?;
    Ok(Json(load_task(&state.db, id).await?))
}

// ---------------------------------------------------------------------------
// Создание
// ---------------------------------------------------------------------------
#[derive(Debug, Deserialize)]
pub struct CreateTask {
    title: Option<String>,
    #[serde(rename = "assigneeId")]
    assignee_id: Option<FlexId>,
    due: Option<String>,
    priority: Option<String>,
    description: Option<String>,
}

fn validated_title(raw: &str) -> ApiResult<String> {
    let title = raw.trim().to_string();
    let len = title.chars().count();
    if len == 0 || len > 300 {
        return Err(ApiError::bad_request("Название задачи: от 1 до 300 символов"));
    }
    Ok(title)
}

fn validated_description(raw: &str) -> ApiResult<String> {
    let text = raw.trim().to_string();
    if text.chars().count() > 10_000 {
        return Err(ApiError::bad_request("Описание слишком длинное (максимум 10 000 символов)"));
    }
    Ok(text)
}

fn validated_priority(raw: &str) -> ApiResult<String> {
    if PRIORITIES.contains(&raw) {
        Ok(raw.to_string())
    } else {
        Err(ApiError::bad_request("Приоритет: Обычный, Высокий или Срочный"))
    }
}

pub async fn create_task(
    State(state): State<AppState>,
    Extension(auth): Extension<CurrentUser>,
    Json(body): Json<CreateTask>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let user = current_user(&state.db, &auth).await?;
    let offset = app_offset(&state.db).await;

    let title = validated_title(body.title.as_deref().unwrap_or(""))?;
    let description = validated_description(body.description.as_deref().unwrap_or(""))?;
    let priority = validated_priority(body.priority.as_deref().unwrap_or("Обычный"))?;

    let assignee: Option<i64> = match &body.assignee_id {
        None => None,
        Some(a) if a.is_blank() => None,
        Some(a) => {
            let id = a.as_i64().ok_or_else(|| ApiError::bad_request("Некорректный ответственный"))?;
            if !employee_exists(&state.db, id).await? {
                return Err(ApiError::bad_request("Сотрудник не найден"));
            }
            Some(id)
        }
    };

    let due = match body.due.as_deref() {
        Some(text) if !text.trim().is_empty() => {
            parse_due(text, &offset).ok_or_else(|| ApiError::bad_request("Не удалось разобрать срок задачи"))?
        }
        _ => {
            let tomorrow = Utc::now().with_timezone(&offset).date_naive() + Duration::days(1);
            match tomorrow.and_hms_opt(12, 0, 0) {
                Some(naive) => local_to_utc(&offset, naive),
                None => Utc::now() + Duration::days(1),
            }
        }
    };

    let (task_id,): (i64,) = sqlx::query_as(
        "INSERT INTO tasks (title, description, assignee_id, assignee, due_at, status, priority, source)
         VALUES ($1, $2, $3, (SELECT name FROM employees WHERE id = $3), $4, 'planned', $5, 'Вручную')
         RETURNING id",
    )
    .bind(&title)
    .bind(&description)
    .bind(assignee)
    .bind(due)
    .bind(&priority)
    .fetch_one(&state.db)
    .await?;

    sqlx::query("INSERT INTO task_history (task_id, text) VALUES ($1, $2)")
        .bind(task_id)
        .bind(format!("Задача создана: {}", user.name))
        .execute(&state.db)
        .await?;

    if let Some(assignee_id) = assignee {
        if assignee_id != user.id {
            let notice = NewNotice::new(
                "Вам назначена задача",
                format!("«{}» · срок: {}", title, fmt_local(due, &offset)),
                "Задачи",
                "Открыть задачу",
                format!("task.html?id={}", task_id),
                false,
            );
            log_notify_error(notify_one(&state.db, assignee_id, &notice).await);
        }
    }

    Ok((StatusCode::CREATED, Json(load_task(&state.db, task_id).await?)))
}

// ---------------------------------------------------------------------------
// Изменение
// ---------------------------------------------------------------------------
#[derive(Debug, Deserialize)]
pub struct ChecklistIn {
    id: Option<FlexId>,
    text: String,
    #[serde(default)]
    done: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateTask {
    title: Option<String>,
    #[serde(rename = "assigneeId")]
    assignee_id: Option<FlexId>,
    due: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    description: Option<String>,
    checklist: Option<Vec<ChecklistIn>>,
}

fn sanitize_checklist(items: &[ChecklistIn]) -> ApiResult<Value> {
    if items.len() > 50 {
        return Err(ApiError::bad_request("В списке шагов не больше 50 пунктов"));
    }
    let mut out = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let text = item.text.trim();
        if text.is_empty() {
            continue;
        }
        if text.chars().count() > 300 {
            return Err(ApiError::bad_request("Шаг не длиннее 300 символов"));
        }
        let id = match &item.id {
            Some(id) if !id.is_blank() => id.to_text(),
            _ => format!("c{}", index + 1),
        };
        out.push(json!({ "id": id, "text": text, "done": item.done }));
    }
    Ok(Value::Array(out))
}

pub async fn update_task(
    State(state): State<AppState>,
    Extension(auth): Extension<CurrentUser>,
    Path(id_raw): Path<String>,
    Json(body): Json<UpdateTask>,
) -> ApiResult<Json<Value>> {
    let user = current_user(&state.db, &auth).await?;
    let id = parse_id(&id_raw, "Задача")?;
    let offset = app_offset(&state.db).await;

    let mut tx = state.db.begin().await?;
    let old = sqlx::query_as::<_, TaskRow>(&format!("{} WHERE t.id = $1 FOR UPDATE OF t", TASK_SELECT))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::not_found("Задача не найдена"))?;

    let mut title = old.title.clone();
    let mut description = old.description.clone();
    let mut assignee_id = old.assignee_id;
    let mut due = old.due_at;
    let mut status = old.status.clone();
    let mut priority = old.priority.clone();
    let mut checklist = old.checklist.clone();
    let mut history: Vec<String> = Vec::new();
    let mut new_assignee_name: Option<String> = None;

    if let Some(raw) = &body.title {
        let value = validated_title(raw)?;
        if value != title {
            history.push(format!("Название изменено: {}", value));
            title = value;
        }
    }
    if let Some(raw) = &body.description {
        let value = validated_description(raw)?;
        if value != description {
            history.push("Описание обновлено".to_string());
            description = value;
        }
    }
    if let Some(a) = &body.assignee_id {
        let target: Option<i64> = if a.is_blank() {
            None
        } else {
            Some(a.as_i64().ok_or_else(|| ApiError::bad_request("Некорректный ответственный"))?)
        };
        if target != assignee_id {
            match target {
                Some(new_id) => {
                    let name: Option<String> = sqlx::query_scalar("SELECT name FROM employees WHERE id = $1")
                        .bind(new_id)
                        .fetch_optional(&mut *tx)
                        .await?;
                    let name = name.ok_or_else(|| ApiError::bad_request("Сотрудник не найден"))?;
                    history.push(format!("Ответственный: {}", name));
                    new_assignee_name = Some(name);
                }
                None => history.push("Ответственный снят".to_string()),
            }
            assignee_id = target;
        }
    }
    if let Some(raw) = &body.status {
        if !STATUSES.contains(&raw.as_str()) {
            return Err(ApiError::bad_request("Неизвестный статус задачи"));
        }
        if *raw != status {
            history.push(format!("Статус изменён: {}", status_label(raw)));
            status = raw.clone();
        }
    }
    if let Some(raw) = &body.priority {
        let value = validated_priority(raw)?;
        if value != priority {
            history.push(format!("Приоритет: {}", value));
            priority = value;
        }
    }
    let mut due_changed = false;
    if let Some(raw) = &body.due {
        let value = parse_due(raw, &offset).ok_or_else(|| ApiError::bad_request("Не удалось разобрать срок задачи"))?;
        if value != due {
            history.push(format!("Срок изменён: {}", fmt_local(value, &offset)));
            due = value;
            due_changed = true;
        }
    }
    if let Some(items) = &body.checklist {
        checklist = sanitize_checklist(items)?;
    }

    sqlx::query(
        "UPDATE tasks SET
            title = $2, description = $3, assignee_id = $4,
            assignee = COALESCE((SELECT name FROM employees WHERE id = $4), assignee),
            due_at = $5, status = $6, priority = $7, checklist = $8,
            completed_at = CASE
                WHEN $6::text = 'done' AND status <> 'done' THEN NOW()
                WHEN $6::text <> 'done' THEN NULL
                ELSE completed_at END,
            updated_at = NOW()
         WHERE id = $1",
    )
    .bind(id)
    .bind(&title)
    .bind(&description)
    .bind(assignee_id)
    .bind(due)
    .bind(&status)
    .bind(&priority)
    .bind(&checklist)
    .execute(&mut *tx)
    .await?;

    for text in &history {
        sqlx::query("INSERT INTO task_history (task_id, text) VALUES ($1, $2)")
            .bind(id)
            .bind(text)
            .execute(&mut *tx)
            .await?;
    }

    if due_changed {
        // напоминание «за N минут до срока» переезжает вместе со сроком и сработает заново
        sqlx::query(
            "UPDATE reminders
             SET remind_at = $2 - (offset_minutes * INTERVAL '1 minute'), is_sent = FALSE, sent_at = NULL
             WHERE task_id = $1",
        )
        .bind(id)
        .bind(due)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;

    // Уведомления — после коммита, ошибки не ломают ответ.
    if let Some(new_id) = assignee_id {
        if old.assignee_id != Some(new_id) && new_id != user.id {
            let notice = NewNotice::new(
                "Вам назначена задача",
                format!("«{}» · срок: {}", title, fmt_local(due, &offset)),
                "Задачи",
                "Открыть задачу",
                format!("task.html?id={}", id),
                false,
            );
            log_notify_error(notify_one(&state.db, new_id, &notice).await);
        }
    }
    if status == "done" && old.status != "done" {
        let who = new_assignee_name
            .clone()
            .or_else(|| old.assignee_name.clone())
            .unwrap_or_else(|| user.name.clone());
        let notice = NewNotice::new(
            "Задача завершена",
            format!("«{}» выполнена. Ответственный: {}.", title, who),
            "Задачи",
            "Подробнее",
            format!("task.html?id={}", id),
            false,
        );
        log_notify_error(notify_managers(&state.db, Some(user.id), &notice).await);
    }

    Ok(Json(load_task(&state.db, id).await?))
}

pub async fn delete_task(
    State(state): State<AppState>,
    Extension(auth): Extension<CurrentUser>,
    Path(id_raw): Path<String>,
) -> ApiResult<Json<Value>> {
    let user = current_user(&state.db, &auth).await?;
    require_manager(&user)?;
    let id = parse_id(&id_raw, "Задача")?;
    let result = sqlx::query("DELETE FROM tasks WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    if result.rows_affected() == 0 {
        return Err(ApiError::not_found("Задача не найдена"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Комментарии
// ---------------------------------------------------------------------------
#[derive(Debug, Deserialize)]
pub struct CommentIn {
    text: Option<String>,
}

pub async fn add_comment(
    State(state): State<AppState>,
    Extension(auth): Extension<CurrentUser>,
    Path(id_raw): Path<String>,
    Json(body): Json<CommentIn>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let user = current_user(&state.db, &auth).await?;
    let id = parse_id(&id_raw, "Задача")?;
    let text = body.text.unwrap_or_default().trim().to_string();
    if text.is_empty() {
        return Err(ApiError::bad_request("Комментарий не может быть пустым"));
    }
    if text.chars().count() > 5000 {
        return Err(ApiError::bad_request("Комментарий слишком длинный (максимум 5000 символов)"));
    }
    let task: Option<(String, Option<i64>)> = sqlx::query_as("SELECT title, assignee_id FROM tasks WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    let (title, assignee) = task.ok_or_else(|| ApiError::not_found("Задача не найдена"))?;

    let (comment_id, at): (i64, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO task_comments (task_id, author_id, author_name, is_ai, text)
         VALUES ($1, $2, $3, FALSE, $4) RETURNING id, created_at",
    )
    .bind(id)
    .bind(user.id)
    .bind(&user.name)
    .bind(&text)
    .fetch_one(&state.db)
    .await?;

    if let Some(assignee_id) = assignee {
        if assignee_id != user.id {
            let preview: String = text.chars().take(120).collect();
            let notice = NewNotice::new(
                "Новый комментарий к задаче",
                format!("{} · «{}»: {}", user.name, title, preview),
                "Задачи",
                "Открыть задачу",
                format!("task.html?id={}", id),
                false,
            );
            log_notify_error(notify_one(&state.db, assignee_id, &notice).await);
        }
    }

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": comment_id,
            "authorName": user.name,
            "isAi": false,
            "text": text,
            "at": at,
        })),
    ))
}

// ---------------------------------------------------------------------------
// Напоминание
// ---------------------------------------------------------------------------
#[derive(Debug, Deserialize)]
pub struct ReminderIn {
    before: Option<String>,
    channel: Option<String>,
}

pub async fn set_reminder(
    State(state): State<AppState>,
    Extension(auth): Extension<CurrentUser>,
    Path(id_raw): Path<String>,
    Json(body): Json<ReminderIn>,
) -> ApiResult<Json<Value>> {
    current_user(&state.db, &auth).await?;
    let id = parse_id(&id_raw, "Задача")?;

    let before = body.before.unwrap_or_else(|| "1h".to_string());
    let (minutes, label) = before_info(&before).ok_or_else(|| ApiError::bad_request("Неизвестный интервал напоминания"))?;
    let channel = body.channel.unwrap_or_else(|| CHANNELS[0].to_string());
    if !CHANNELS.contains(&channel.as_str()) {
        return Err(ApiError::bad_request("Неизвестный канал напоминания"));
    }

    let task: Option<(DateTime<Utc>, String)> = sqlx::query_as("SELECT due_at, status FROM tasks WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    let (due, status) = task.ok_or_else(|| ApiError::not_found("Задача не найдена"))?;
    if status == "done" {
        return Err(ApiError::bad_request("Задача уже завершена — напоминание не нужно"));
    }
    let remind_at = due - Duration::minutes(minutes as i64);
    if remind_at < Utc::now() {
        return Err(ApiError::bad_request(
            "Это время уже прошло: выберите более короткий интервал или измените срок задачи",
        ));
    }

    let text = format!("{} · {}", label, channel);
    sqlx::query(
        "INSERT INTO reminders (task_id, text, remind_at, is_sent, before_code, offset_minutes, channel, sent_at)
         VALUES ($1, $2, $3, FALSE, $4, $5, $6, NULL)
         ON CONFLICT (task_id) WHERE task_id IS NOT NULL DO UPDATE SET
            text = EXCLUDED.text, remind_at = EXCLUDED.remind_at, is_sent = FALSE,
            before_code = EXCLUDED.before_code, offset_minutes = EXCLUDED.offset_minutes,
            channel = EXCLUDED.channel, sent_at = NULL",
    )
    .bind(id)
    .bind(&text)
    .bind(remind_at)
    .bind(&before)
    .bind(minutes)
    .bind(&channel)
    .execute(&state.db)
    .await?;

    sqlx::query("INSERT INTO task_history (task_id, text) VALUES ($1, $2)")
        .bind(id)
        .bind(format!("Напоминание: {}", text))
        .execute(&state.db)
        .await?;

    Ok(Json(load_task(&state.db, id).await?))
}

// ---------------------------------------------------------------------------
// Задачи из протокола встречи (вызывается после confirm_meeting)
// ---------------------------------------------------------------------------
/// Создаёт задачи из реестра, который составила нейросеть. Ничего не выдумывает:
/// если ответственный или срок не распознаны — задача создаётся с пометкой в описании.
pub async fn create_from_protocol(
    db: &PgPool,
    meeting_id: i64,
    meeting_title: &str,
    items: &[crate::TaskItem],
) -> Result<usize, sqlx::Error> {
    if items.is_empty() {
        return Ok(0);
    }
    let settings = load_settings(db).await;
    let offset: FixedOffset = settings_offset(&settings);
    let send_reminders = settings["sendReminders"].as_bool().unwrap_or(true);
    let now_local = Utc::now().with_timezone(&offset);
    let today = (now_local.year() as i64, now_local.month(), now_local.day());
    let weekday = now_local.weekday().num_days_from_monday();

    let mut tx = db.begin().await?;
    let mut created = 0usize;
    let mut unresolved = 0usize;
    // assignee_id -> список названий задач
    let mut per_assignee: HashMap<i64, Vec<String>> = HashMap::new();

    for item in items {
        let title: String = item.formulation.trim().chars().take(300).collect();
        if title.is_empty() {
            continue;
        }

        let owner = item.owner.trim().to_string();
        let owner_known = !owner.is_empty() && !owner.to_lowercase().contains("уточн");
        let assignee: Option<(i64, String)> = if owner_known {
            sqlx::query_as(
                "SELECT id, name FROM employees
                 WHERE LOWER(name) = LOWER($1) OR POSITION(LOWER($1) IN LOWER(name)) > 0
                 ORDER BY (LOWER(name) = LOWER($1)) DESC, id LIMIT 1",
            )
            .bind(&owner)
            .fetch_optional(&mut *tx)
            .await?
        } else {
            None
        };

        let parsed = util::parse_ru_deadline(&item.deadline, today, weekday).and_then(|(y, m, d, h, mi)| {
            offset
                .with_ymd_and_hms(y as i32, m, d, h, mi, 0)
                .single()
                .map(|dt| dt.with_timezone(&Utc))
        });
        let deadline_known = parsed.is_some();
        let due = parsed.unwrap_or_else(|| Utc::now() + Duration::days(1));

        let mut notes: Vec<String> = Vec::new();
        if assignee.is_none() {
            notes.push(format!(
                "Ответственный не определён ({}). Назначьте сотрудника.",
                if owner.is_empty() { "в протоколе не указан" } else { owner.as_str() }
            ));
        }
        if !deadline_known {
            notes.push(format!(
                "Срок в протоколе не определён ({}). Временно установлен через сутки — уточните.",
                if item.deadline.trim().is_empty() { "не указан" } else { item.deadline.trim() }
            ));
        }
        if assignee.is_none() || !deadline_known {
            unresolved += 1;
        }

        let basis = item.basis.trim();
        let (quote_title, quote_text) = if basis.is_empty() {
            (None, None)
        } else {
            (
                Some(format!("{} · основание", meeting_title)),
                Some(format!("«{}»", basis)),
            )
        };

        let (task_id,): (i64,) = sqlx::query_as(
            "INSERT INTO tasks
                (meeting_id, title, description, assignee_id, assignee, due_at, status, priority,
                 source, quote_title, quote_text, due_text, basis)
             VALUES ($1, $2, $3, $4, $5, $6, 'planned', 'Обычный', $7, $8, $9, $10, $11)
             RETURNING id",
        )
        .bind(meeting_id)
        .bind(&title)
        .bind(notes.join("\n"))
        .bind(assignee.as_ref().map(|a| a.0))
        .bind(if owner.is_empty() { None } else { Some(owner.clone()) })
        .bind(due)
        .bind(meeting_title)
        .bind(&quote_title)
        .bind(&quote_text)
        .bind(item.deadline.trim())
        .bind(basis)
        .fetch_one(&mut *tx)
        .await?;

        let comment = match (&assignee, deadline_known) {
            (Some((_, name)), true) => format!(
                "Создал задачу из встречи, назначил {} и установил срок: {}.",
                name,
                fmt_local(due, &offset)
            ),
            _ => "Создал задачу из встречи. Ответственный или срок требуют уточнения — подробности в описании."
                .to_string(),
        };
        sqlx::query(
            "INSERT INTO task_comments (task_id, author_name, is_ai, text) VALUES ($1, 'Фотопринт AI', TRUE, $2)",
        )
        .bind(task_id)
        .bind(comment)
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO task_history (task_id, text) VALUES ($1, $2)")
            .bind(task_id)
            .bind(format!("Задача создана AI из встречи «{}»", meeting_title))
            .execute(&mut *tx)
            .await?;

        // автонапоминание за час до срока — только если срок и ответственный известны
        if send_reminders && assignee.is_some() && deadline_known {
            let remind_at = due - Duration::minutes(60);
            if remind_at > Utc::now() {
                sqlx::query(
                    "INSERT INTO reminders (task_id, text, remind_at, is_sent, before_code, offset_minutes, channel)
                     VALUES ($1, $2, $3, FALSE, '1h', 60, 'Telegram и почта')",
                )
                .bind(task_id)
                .bind("За час до срока · Telegram и почта")
                .bind(remind_at)
                .execute(&mut *tx)
                .await?;
            }
        }

        if let Some((assignee_id, _)) = &assignee {
            per_assignee.entry(*assignee_id).or_default().push(title.clone());
        }
        created += 1;
    }
    tx.commit().await?;

    if created > 0 {
        let text = if unresolved == 0 {
            "У каждой — ответственный и срок. Проверьте результат перед началом работы.".to_string()
        } else {
            format!(
                "Для {} из них нужно уточнить ответственного или срок — это отмечено в описании.",
                unresolved
            )
        };
        let notice = NewNotice::new(
            format!(
                "Созданы {} {} из встречи",
                created,
                util::plural(created as i64, "задача", "задачи", "задач")
            ),
            format!("«{}». {}", meeting_title, text),
            "Задачи",
            "Открыть задачи",
            format!("tasks.html?meetingId={}", meeting_id),
            true,
        );
        log_notify_error(notify_managers(db, None, &notice).await);

        for (assignee_id, titles) in per_assignee {
            let notice = NewNotice::new(
                format!(
                    "Вам назначено {} {}",
                    titles.len(),
                    util::plural(titles.len() as i64, "задача", "задачи", "задач")
                ),
                format!("Встреча «{}»: {}", meeting_title, titles.join("; ")),
                "Задачи",
                "Открыть задачи",
                "tasks.html".to_string(),
                true,
            );
            log_notify_error(notify_one(db, assignee_id, &notice).await);
        }
    }
    Ok(created)
}
