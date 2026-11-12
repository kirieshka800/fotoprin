//! Напоминания: лента для страницы «Задачи», ближайшее напоминание для «Уведомлений»
//! и фоновый воркер, который доставляет сработавшие напоминания.

use axum::{extract::State, Json};
use chrono::{DateTime, Datelike, FixedOffset, TimeZone, Timelike, Utc};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::time::Duration;

use super::notifications::{log_notify_error, notify_managers, notify_one, NewNotice};
use super::*;
use crate::AppState;

fn day_start_utc(now: DateTime<Utc>, offset: &FixedOffset) -> DateTime<Utc> {
    let local_date = now.with_timezone(offset).date_naive();
    match local_date.and_hms_opt(0, 0, 0) {
        Some(naive) => local_to_utc(offset, naive),
        None => now - chrono::Duration::hours(24),
    }
}

/// «12:30» для сегодняшнего дня, иначе «12 ноя» (+ «, 12:30», если `with_clock`).
fn time_label(at: DateTime<Utc>, offset: &FixedOffset, with_clock: bool) -> String {
    let local = at.with_timezone(offset);
    let today = Utc::now().with_timezone(offset).date_naive();
    let clock = format!("{:02}:{:02}", local.hour(), local.minute());
    if local.date_naive() == today {
        clock
    } else {
        let day = format!("{} {}", local.day(), util::MONTHS_SHORT[local.month0() as usize]);
        if with_clock {
            format!("{}, {}", day, clock)
        } else {
            day
        }
    }
}

type QueuedRow = (i64, DateTime<Utc>, String, String, Option<String>);

/// Руководитель видит все напоминания, сотрудник — только по своим задачам.
async fn queued(db: &PgPool, user: &Employee, limit: i64) -> Result<Vec<QueuedRow>, sqlx::Error> {
    let scope: Option<i64> = if user.is_manager() { None } else { Some(user.id) };
    sqlx::query_as(
        "SELECT r.id, r.remind_at, r.channel, t.title, e.name
         FROM reminders r
         JOIN tasks t ON t.id = r.task_id
         LEFT JOIN employees e ON e.id = t.assignee_id
         WHERE r.is_sent = FALSE AND t.status <> 'done'
           AND ($1::bigint IS NULL OR t.assignee_id = $1)
         ORDER BY r.remind_at, r.id
         LIMIT $2",
    )
    .bind(scope)
    .bind(limit)
    .fetch_all(db)
    .await
}

fn meta_text(name: &Option<String>, channel: &str) -> String {
    match name {
        Some(n) => format!("{} · {}", n, channel),
        None => channel.to_string(),
    }
}

pub async fn next_reminder(
    db: &PgPool,
    user: &Employee,
    offset: &FixedOffset,
) -> Result<Option<Value>, sqlx::Error> {
    let rows = queued(db, user, 1).await?;
    Ok(rows.into_iter().next().map(|(_, at, channel, title, name)| {
        json!({
            "time": time_label(at, offset, true),
            "title": title,
            "meta": meta_text(&name, &channel),
        })
    }))
}

pub async fn list_reminders(State(state): State<AppState>, Extension(auth): Extension<CurrentUser>) -> ApiResult<Json<Value>> {
    let user = current_user(&state.db, &auth).await?;
    let offset = app_offset(&state.db).await;
    let mut feed: Vec<Value> = Vec::new();

    for (id, at, channel, title, name) in queued(&state.db, &user, 3).await? {
        feed.push(json!({
            "id": format!("q{}", id),
            "time": time_label(at, &offset, false),
            "title": title,
            "meta": meta_text(&name, &channel),
            "state": "В очереди",
            "tone": "purple",
        }));
    }

    let since = day_start_utc(Utc::now(), &offset);
    let scope: Option<i64> = if user.is_manager() { None } else { Some(user.id) };
    let sent: (i64, Option<DateTime<Utc>>) = sqlx::query_as(
        "SELECT COUNT(*), MAX(r.sent_at)
         FROM reminders r JOIN tasks t ON t.id = r.task_id
         WHERE r.is_sent = TRUE AND r.sent_at >= $1
           AND ($2::bigint IS NULL OR t.assignee_id = $2)",
    )
    .bind(since)
    .bind(scope)
    .fetch_one(&state.db)
    .await?;
    if sent.0 > 0 {
        let at = sent.1.unwrap_or_else(Utc::now);
        let meta = if user.is_manager() { "Сотрудникам · в приложении" } else { "Вам · в приложении" };
        feed.push(json!({
            "id": "sent-today",
            "time": time_label(at, &offset, false),
            "title": format!("{} {} о задачах", sent.0, util::plural(sent.0, "напоминание", "напоминания", "напоминаний")),
            "meta": meta,
            "state": "Доставлены",
            "tone": "green",
        }));
    }

    if user.is_manager() && schedule::pending_counts(&state.db).await?.is_some() {
        feed.push(json!({
            "id": "schedule-publish",
            "time": "После",
            "title": "Новый график смен",
            "meta": "Команде · после утверждения",
            "state": "На паузе",
            "tone": "orange",
        }));
    }
    Ok(Json(Value::Array(feed)))
}

// ---------------------------------------------------------------------------
// Воркер
// ---------------------------------------------------------------------------
#[derive(Debug, sqlx::FromRow)]
struct DueRow {
    id: i64,
    task_id: i64,
    title: String,
    due_at: DateTime<Utc>,
    assignee_id: Option<i64>,
}

/// Раз в полминуты доставляет сработавшие напоминания.
/// Запускается из main: tokio::spawn(api::reminders::run_worker(db.clone())).
pub async fn run_worker(db: PgPool) {
    loop {
        if let Err(err) = process_due(&db).await {
            eprintln!("Воркер напоминаний: {}", err);
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

async fn process_due(db: &PgPool) -> Result<(), sqlx::Error> {
    let settings = load_settings(db).await;
    if !settings["sendReminders"].as_bool().unwrap_or(true) {
        return Ok(());
    }
    let offset = settings_offset(&settings);

    // Тихие часы: несрочные напоминания ждут утра, «Срочный» приоритет их обходит.
    let quiet = &settings["quietHours"];
    let in_quiet = if quiet["enabled"].as_bool().unwrap_or(false) {
        let from = quiet["from"].as_str().and_then(util::parse_hhmm);
        let to = quiet["to"].as_str().and_then(util::parse_hhmm);
        match (from, to) {
            (Some(from), Some(to)) => {
                let local = Utc::now().with_timezone(&offset);
                util::in_quiet_hours(local.hour() * 60 + local.minute(), from, to)
            }
            _ => false,
        }
    } else {
        false
    };

    let due = sqlx::query_as::<_, DueRow>(
        "SELECT r.id, t.id AS task_id, t.title, t.due_at, t.assignee_id
         FROM reminders r JOIN tasks t ON t.id = r.task_id
         WHERE r.is_sent = FALSE AND r.remind_at <= NOW() AND t.status <> 'done'
           AND ($1 OR t.priority = 'Срочный')
         ORDER BY r.remind_at
         LIMIT 100",
    )
    .bind(!in_quiet)
    .fetch_all(db)
    .await?;

    for row in due {
        // «захватываем» напоминание: если параллельно его уже отправили, rows_affected будет 0
        let claimed = sqlx::query(
            "UPDATE reminders SET is_sent = TRUE, sent_at = NOW() WHERE id = $1 AND is_sent = FALSE",
        )
        .bind(row.id)
        .execute(db)
        .await?;
        if claimed.rows_affected() == 0 {
            continue;
        }
        let notice = NewNotice::new(
            "Напоминание о задаче",
            format!("«{}» · срок: {}", row.title, fmt_local(row.due_at, &offset)),
            "Напоминания",
            "Открыть задачу",
            format!("task.html?id={}", row.task_id),
            true,
        );
        match row.assignee_id {
            Some(assignee) => log_notify_error(notify_one(db, assignee, &notice).await),
            None => log_notify_error(notify_managers(db, None, &notice).await),
        }
    }
    Ok(())
}
