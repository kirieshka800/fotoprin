//! График работы: смены, предложение ассистента (Ollama), отклонение и публикация.
//!
//! Модель данных: смена хранит время в минутах от начала суток. Предложение AI — это смены
//! со статусом `ai`; для изменённой смены в prev_* лежит прежнее время («было»).
//! Отклонение возвращает prev_* (или удаляет новую смену), публикация делает смены `confirmed`.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{Datelike, Duration, NaiveDate};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::notifications::{log_notify_error, mark_kind_read, notify_managers, notify_one, NewNotice};
use super::*;
use crate::AppState;

const MAX_AI_CHANGES: usize = 20;
const MIN_REST_MINUTES: i32 = 720;

#[derive(Debug, Clone, sqlx::FromRow)]
struct ShiftRow {
    id: i64,
    employee_id: i64,
    shift_date: NaiveDate,
    start_min: i32,
    end_min: i32,
    note: String,
    status: String,
    prev_start_min: Option<i32>,
    prev_end_min: Option<i32>,
}

const SHIFT_COLS: &str =
    "id, employee_id, shift_date, start_min, end_min, note, status, prev_start_min, prev_end_min";

impl ShiftRow {
    fn time(&self) -> String {
        util::fmt_range(self.start_min, self.end_min)
    }
    fn was(&self) -> String {
        match (self.prev_start_min, self.prev_end_min) {
            (Some(s), Some(e)) => util::fmt_range(s, e),
            _ => String::new(),
        }
    }
    fn to_json(&self) -> Value {
        json!({
            "id": self.id.to_string(),
            "employeeId": self.employee_id,
            "date": self.shift_date.format("%Y-%m-%d").to_string(),
            "time": self.time(),
            "note": self.note,
            "status": self.status,
            "was": self.was(),
        })
    }
}

fn monday_of(date: NaiveDate) -> NaiveDate {
    date - Duration::days(date.weekday().num_days_from_monday() as i64)
}

fn iso(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

fn long_date(date: NaiveDate) -> String {
    format!("{} {}", date.day(), util::MONTHS_FULL[date.month0() as usize])
}

fn week_from(raw: Option<&str>, offset: &chrono::FixedOffset) -> ApiResult<NaiveDate> {
    match raw {
        Some(text) if !text.trim().is_empty() => NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d")
            .map(monday_of)
            .map_err(|_| ApiError::bad_request("weekStart: ожидается дата в формате ГГГГ-ММ-ДД")),
        _ => Ok(monday_of(chrono::Utc::now().with_timezone(offset).date_naive())),
    }
}

async fn ensure_week(db: &PgPool, week: NaiveDate) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO schedule_weeks (week_start) VALUES ($1) ON CONFLICT (week_start) DO NOTHING")
        .bind(week)
        .execute(db)
        .await?;
    Ok(())
}

async fn week_published(db: &PgPool, date: NaiveDate) -> bool {
    let week = monday_of(date);
    let row: Result<Option<bool>, _> =
        sqlx::query_scalar("SELECT published FROM schedule_weeks WHERE week_start = $1")
            .bind(week)
            .fetch_optional(db)
            .await;
    matches!(row, Ok(Some(true)))
}

fn map_shift_err(err: sqlx::Error) -> ApiError {
    if let sqlx::Error::Database(db_err) = &err {
        if db_err.is_unique_violation() {
            return ApiError::conflict("У сотрудника уже есть смена на эту дату");
        }
    }
    ApiError::from(err)
}

// ---------------------------------------------------------------------------
// Проверки графика
// ---------------------------------------------------------------------------
/// (успешные проверки для отображения, предупреждения о нарушениях)
fn schedule_checks(shifts: &[ShiftRow], names: &HashMap<i64, String>) -> (Vec<String>, Vec<String>) {
    let mut by_employee: BTreeMap<i64, BTreeMap<NaiveDate, (i32, i32)>> = BTreeMap::new();
    for s in shifts {
        by_employee
            .entry(s.employee_id)
            .or_default()
            .insert(s.shift_date, (s.start_min, s.end_min));
    }
    let mut overlaps: Vec<String> = Vec::new();
    let mut shorts: Vec<String> = Vec::new();
    for (employee, days) in &by_employee {
        let name = names.get(employee).cloned().unwrap_or_default();
        for (date, (start, end)) in days {
            let next = *date + Duration::days(1);
            if let Some((next_start, _)) = days.get(&next) {
                let rest = util::rest_minutes(*start, *end, *next_start);
                if rest < 0 {
                    overlaps.push(format!("{}: смены {} и {} пересекаются", name, long_date(*date), long_date(next)));
                } else if rest < MIN_REST_MINUTES {
                    shorts.push(format!(
                        "{}: между сменами {} и {} меньше 12 часов",
                        name,
                        long_date(*date),
                        long_date(next)
                    ));
                }
            }
        }
    }
    let mut checks = Vec::new();
    if !shifts.is_empty() {
        if overlaps.is_empty() {
            checks.push("Пересечений смен не найдено".to_string());
            if shorts.is_empty() {
                checks.push("Между сменами — не менее 12 часов".to_string());
            }
        }
    }
    let mut warnings = overlaps;
    warnings.extend(shorts);
    (checks, warnings)
}

/// Новая смена не должна пересекаться со сменами соседних дней этого же сотрудника.
async fn check_neighbors(
    db: &PgPool,
    employee: i64,
    date: NaiveDate,
    start: i32,
    end: i32,
    exclude: Option<i64>,
) -> ApiResult<()> {
    let rows: Vec<(NaiveDate, i32, i32)> = sqlx::query_as(
        "SELECT shift_date, start_min, end_min FROM shifts
         WHERE employee_id = $1 AND shift_date IN ($2, $3)
           AND ($4::bigint IS NULL OR id <> $4)",
    )
    .bind(employee)
    .bind(date - Duration::days(1))
    .bind(date + Duration::days(1))
    .bind(exclude)
    .fetch_all(db)
    .await?;
    for (other_date, other_start, other_end) in rows {
        let rest = if other_date < date {
            util::rest_minutes(other_start, other_end, start)
        } else {
            util::rest_minutes(start, end, other_start)
        };
        if rest < 0 {
            return Err(ApiError::conflict("Смена пересекается со сменой соседнего дня"));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Предложение AI: сводки
// ---------------------------------------------------------------------------
async fn proposal_for_week(db: &PgPool, week: NaiveDate) -> Result<Value, sqlx::Error> {
    let end = week + Duration::days(6);
    let rows: Vec<(NaiveDate, String, i32, i32, Option<i32>, Option<i32>)> = sqlx::query_as(
        "SELECT s.shift_date, e.name, s.start_min, s.end_min, s.prev_start_min, s.prev_end_min
         FROM shifts s JOIN employees e ON e.id = s.employee_id
         WHERE s.status = 'ai' AND s.shift_date BETWEEN $1 AND $2
         ORDER BY s.shift_date, e.name",
    )
    .bind(week)
    .bind(end)
    .fetch_all(db)
    .await?;
    if rows.is_empty() {
        return Ok(Value::Null);
    }
    let stored: Option<String> =
        sqlx::query_scalar("SELECT proposal_text FROM schedule_weeks WHERE week_start = $1")
            .bind(week)
            .fetch_optional(db)
            .await?
            .flatten();
    let count = rows.len() as i64;
    let text = stored.unwrap_or_else(|| {
        format!(
            "AI предлагает изменить {} {}",
            count,
            util::plural(count, "смену", "смены", "смен")
        )
    });
    let changes: Vec<Value> = rows
        .iter()
        .map(|(date, who, start, end, prev_start, prev_end)| {
            let was = match (prev_start, prev_end) {
                (Some(s), Some(e)) => util::fmt_range(*s, *e),
                _ => String::new(),
            };
            json!({ "who": who, "date": iso(*date), "was": was, "now": util::fmt_range(*start, *end) })
        })
        .collect();
    Ok(json!({ "text": text, "count": count, "changes": changes }))
}

pub async fn pending_counts(db: &PgPool) -> Result<Option<(i64, i64)>, sqlx::Error> {
    let (people, shifts): (i64, i64) =
        sqlx::query_as("SELECT COUNT(DISTINCT employee_id), COUNT(*) FROM shifts WHERE status = 'ai'")
            .fetch_one(db)
            .await?;
    Ok(if shifts > 0 { Some((people, shifts)) } else { None })
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Блок «Нужно подтверждение» на странице уведомлений: сводка по всем неутверждённым предложениям.
pub async fn pending_summary(db: &PgPool) -> Result<Option<Value>, sqlx::Error> {
    let rows: Vec<(NaiveDate, String, i32, i32, Option<i32>, Option<i32>)> = sqlx::query_as(
        "SELECT s.shift_date, e.name, s.start_min, s.end_min, s.prev_start_min, s.prev_end_min
         FROM shifts s JOIN employees e ON e.id = s.employee_id
         WHERE s.status = 'ai'
         ORDER BY s.shift_date, e.name",
    )
    .fetch_all(db)
    .await?;
    if rows.is_empty() {
        return Ok(None);
    }

    let mut dates: BTreeSet<NaiveDate> = BTreeSet::new();
    let mut who: Vec<String> = Vec::new();
    let mut was: Vec<String> = Vec::new();
    let mut now: Vec<String> = Vec::new();
    for (date, name, start, end, prev_start, prev_end) in &rows {
        dates.insert(*date);
        if !who.contains(name) {
            who.push(name.clone());
        }
        if let (Some(s), Some(e)) = (prev_start, prev_end) {
            let text = util::fmt_range(*s, *e);
            if !was.contains(&text) {
                was.push(text);
            }
        }
        let text = util::fmt_range(*start, *end);
        if !now.contains(&text) {
            now.push(text);
        }
    }

    let day_names: Vec<String> = dates
        .iter()
        .map(|d| util::WEEKDAYS_NOM[d.weekday().num_days_from_monday() as usize].to_string())
        .collect();
    let mut unique_names: Vec<String> = Vec::new();
    for n in day_names {
        if !unique_names.contains(&n) {
            unique_names.push(n);
        }
    }
    let names_text = match unique_names.len() {
        1 => unique_names[0].clone(),
        n => format!("{} и {}", unique_names[..n - 1].join(", "), unique_names[n - 1]),
    };
    let first = *dates.iter().next().unwrap_or(&NaiveDate::MIN);
    let last = *dates.iter().next_back().unwrap_or(&first);
    let range = if first == last {
        long_date(first)
    } else if first.month() == last.month() {
        format!("{}–{} {}", first.day(), last.day(), util::MONTHS_FULL[last.month0() as usize])
    } else {
        format!("{} – {}", long_date(first), long_date(last))
    };

    let was_text = if was.is_empty() { "—".to_string() } else { was.join(", ") };
    Ok(Some(json!({
        "title": "Изменения графика",
        "text": format!("{}, {}", capitalize(&names_text), range),
        "who": who.join(" · "),
        "was": was_text,
        "now": now.join(", "),
    })))
}

// ---------------------------------------------------------------------------
// Чтение недели
// ---------------------------------------------------------------------------
#[derive(Debug, Deserialize)]
pub struct WeekQuery {
    #[serde(rename = "weekStart")]
    week_start: Option<String>,
}

pub async fn get_schedule(
    State(state): State<AppState>,
    Extension(auth): Extension<CurrentUser>,
    Query(query): Query<WeekQuery>,
) -> ApiResult<Json<Value>> {
    current_user(&state.db, &auth).await?;
    let offset = app_offset(&state.db).await;
    let week = week_from(query.week_start.as_deref(), &offset)?;
    let end = week + Duration::days(6);

    let employees = sqlx::query_as::<_, Employee>(&format!(
        "SELECT {} FROM employees ORDER BY (role = 'Руководитель') DESC, name, id",
        EMPLOYEE_COLS
    ))
    .fetch_all(&state.db)
    .await?;
    let names: HashMap<i64, String> = employees.iter().map(|e| (e.id, e.name.clone())).collect();

    // день до начала недели нужен только для проверки отдыха между сменами
    let all = sqlx::query_as::<_, ShiftRow>(&format!(
        "SELECT {} FROM shifts WHERE shift_date BETWEEN $1 AND $2 ORDER BY shift_date, employee_id",
        SHIFT_COLS
    ))
    .bind(week - Duration::days(1))
    .bind(end)
    .fetch_all(&state.db)
    .await?;
    let in_week: Vec<&ShiftRow> = all.iter().filter(|s| s.shift_date >= week).collect();

    let week_row: Option<(bool, Option<String>, Option<String>, Option<i64>)> = sqlx::query_as(
        "SELECT published, why_text, why_source, why_meeting_id FROM schedule_weeks WHERE week_start = $1",
    )
    .bind(week)
    .fetch_optional(&state.db)
    .await?;
    let (published, why_text, why_source, why_meeting) = week_row.unwrap_or((false, None, None, None));

    let why = match why_text {
        Some(text) => json!({
            "text": text,
            "source": why_source,
            "href": why_meeting.map(|id| format!("meeting.html?id={}", id)),
        }),
        None => Value::Null,
    };

    let (checks, warnings) = schedule_checks(&all, &names);
    let days: Vec<String> = (0..7).map(|i| iso(week + Duration::days(i))).collect();

    let proposal = proposal_for_week(&state.db, week).await?;
    let employees_json: Vec<Value> = employees.iter().map(|e| e.to_json()).collect();
    let shifts_json: Vec<Value> = in_week.iter().map(|s| s.to_json()).collect();

    Ok(Json(json!({
        "weekStart": iso(week),
        "days": days,
        "employees": employees_json,
        "shifts": shifts_json,
        "proposal": proposal,
        "published": published,
        "why": why,
        "checks": checks,
        "warnings": warnings,
    })))
}

// ---------------------------------------------------------------------------
// Смены вручную
// ---------------------------------------------------------------------------
#[derive(Debug, Deserialize)]
pub struct ShiftIn {
    #[serde(rename = "employeeId")]
    employee_id: Option<FlexId>,
    date: Option<String>,
    time: Option<String>,
    note: Option<String>,
}

struct ValidShift {
    employee: i64,
    date: NaiveDate,
    start: i32,
    end: i32,
    note: String,
}

async fn validate_shift(db: &PgPool, body: &ShiftIn) -> ApiResult<ValidShift> {
    let employee = body
        .employee_id
        .as_ref()
        .and_then(|e| e.as_i64())
        .ok_or_else(|| ApiError::bad_request("Выберите сотрудника"))?;
    if !employee_exists(db, employee).await? {
        return Err(ApiError::bad_request("Сотрудник не найден"));
    }
    let date = body
        .date
        .as_deref()
        .and_then(|d| NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d").ok())
        .ok_or_else(|| ApiError::bad_request("Укажите дату смены"))?;
    let (start, end) = body
        .time
        .as_deref()
        .and_then(util::parse_time_range)
        .ok_or_else(|| ApiError::bad_request("Укажите время, например 14:00 — 22:00"))?;
    let note = body.note.clone().unwrap_or_default().trim().to_string();
    if note.chars().count() > 255 {
        return Err(ApiError::bad_request("Примечание не длиннее 255 символов"));
    }
    Ok(ValidShift { employee, date, start, end, note })
}

async fn notify_shift_change(db: &PgPool, employee: i64, title: &str, date: NaiveDate, time: Option<String>) {
    let text = match time {
        Some(t) => format!("{} · {}", long_date(date), t),
        None => long_date(date),
    };
    let notice = NewNotice::new(title, text, "График", "Открыть график", "schedule.html", false);
    log_notify_error(notify_one(db, employee, &notice).await);
}

pub async fn add_shift(
    State(state): State<AppState>,
    Extension(auth): Extension<CurrentUser>,
    Json(body): Json<ShiftIn>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let user = current_user(&state.db, &auth).await?;
    require_manager(&user)?;
    let v = validate_shift(&state.db, &body).await?;
    check_neighbors(&state.db, v.employee, v.date, v.start, v.end, None).await?;
    ensure_week(&state.db, monday_of(v.date)).await?;

    let row = sqlx::query_as::<_, ShiftRow>(&format!(
        "INSERT INTO shifts (employee_id, shift_date, start_min, end_min, note, status)
         VALUES ($1, $2, $3, $4, $5, 'confirmed') RETURNING {}",
        SHIFT_COLS
    ))
    .bind(v.employee)
    .bind(v.date)
    .bind(v.start)
    .bind(v.end)
    .bind(&v.note)
    .fetch_one(&state.db)
    .await
    .map_err(map_shift_err)?;

    if week_published(&state.db, v.date).await && v.employee != user.id {
        notify_shift_change(&state.db, v.employee, "Вам добавлена смена", v.date, Some(row.time())).await;
    }
    Ok((StatusCode::CREATED, Json(row.to_json())))
}

pub async fn update_shift(
    State(state): State<AppState>,
    Extension(auth): Extension<CurrentUser>,
    Path(id_raw): Path<String>,
    Json(body): Json<ShiftIn>,
) -> ApiResult<Json<Value>> {
    let user = current_user(&state.db, &auth).await?;
    require_manager(&user)?;
    let id = parse_id(&id_raw, "Смена")?;
    let old = sqlx::query_as::<_, ShiftRow>(&format!("SELECT {} FROM shifts WHERE id = $1", SHIFT_COLS))
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ApiError::not_found("Смена не найдена"))?;

    let v = validate_shift(&state.db, &body).await?;
    check_neighbors(&state.db, v.employee, v.date, v.start, v.end, Some(id)).await?;
    ensure_week(&state.db, monday_of(v.date)).await?;

    // ручное редактирование снимает пометку «предложение AI»
    let row = sqlx::query_as::<_, ShiftRow>(&format!(
        "UPDATE shifts SET employee_id = $2, shift_date = $3, start_min = $4, end_min = $5, note = $6,
                status = 'confirmed', prev_start_min = NULL, prev_end_min = NULL, updated_at = NOW()
         WHERE id = $1 RETURNING {}",
        SHIFT_COLS
    ))
    .bind(id)
    .bind(v.employee)
    .bind(v.date)
    .bind(v.start)
    .bind(v.end)
    .bind(&v.note)
    .fetch_one(&state.db)
    .await
    .map_err(map_shift_err)?;

    let changed = old.employee_id != row.employee_id
        || old.shift_date != row.shift_date
        || old.start_min != row.start_min
        || old.end_min != row.end_min;
    if changed {
        if week_published(&state.db, row.shift_date).await && row.employee_id != user.id {
            notify_shift_change(&state.db, row.employee_id, "Ваша смена изменена", row.shift_date, Some(row.time())).await;
        }
        if old.employee_id != row.employee_id
            && old.employee_id != user.id
            && week_published(&state.db, old.shift_date).await
        {
            notify_shift_change(&state.db, old.employee_id, "Смена отменена", old.shift_date, None).await;
        }
    }
    Ok(Json(row.to_json()))
}

pub async fn delete_shift(
    State(state): State<AppState>,
    Extension(auth): Extension<CurrentUser>,
    Path(id_raw): Path<String>,
) -> ApiResult<Json<Value>> {
    let user = current_user(&state.db, &auth).await?;
    require_manager(&user)?;
    let id = parse_id(&id_raw, "Смена")?;
    let deleted: Option<(i64, NaiveDate)> =
        sqlx::query_as("DELETE FROM shifts WHERE id = $1 RETURNING employee_id, shift_date")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let (employee, date) = deleted.ok_or_else(|| ApiError::not_found("Смена не найдена"))?;
    if employee != user.id && week_published(&state.db, date).await {
        notify_shift_change(&state.db, employee, "Смена отменена", date, None).await;
    }
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Предложение AI
// ---------------------------------------------------------------------------
#[derive(Debug, Deserialize)]
pub struct WeekBody {
    #[serde(rename = "weekStart")]
    week_start: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AiChange {
    #[serde(rename = "employeeId")]
    employee_id: FlexId,
    date: String,
    start: String,
    end: String,
}

#[derive(Debug, Deserialize)]
struct AiPlan {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    why: String,
    #[serde(default, rename = "sourceMeetingId")]
    source_meeting_id: Option<FlexId>,
    #[serde(default)]
    changes: Vec<AiChange>,
}

/// Возвращает смены недели к утверждённому виду и удаляет добавленные AI.
async fn revert_proposal(tx: &mut Transaction<'_, Postgres>, week: NaiveDate) -> Result<(), sqlx::Error> {
    let end = week + Duration::days(6);
    sqlx::query(
        "UPDATE shifts SET start_min = prev_start_min, end_min = prev_end_min, status = 'confirmed',
                prev_start_min = NULL, prev_end_min = NULL, updated_at = NOW()
         WHERE status = 'ai' AND prev_start_min IS NOT NULL AND prev_end_min IS NOT NULL
           AND shift_date BETWEEN $1 AND $2",
    )
    .bind(week)
    .bind(end)
    .execute(&mut **tx)
    .await?;
    sqlx::query("DELETE FROM shifts WHERE status = 'ai' AND shift_date BETWEEN $1 AND $2")
        .bind(week)
        .bind(end)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

fn protocol_text(summary: &str) -> String {
    match serde_json::from_str::<crate::ProtocolResult>(summary) {
        Ok(p) => {
            let mut lines: Vec<String> = Vec::new();
            for item in &p.items {
                lines.push(format!("- [{}] {}", item.category, item.text));
            }
            for task in &p.tasks {
                lines.push(format!("- Задача: {} (ответственный: {}, срок: {})", task.formulation, task.owner, task.deadline));
            }
            lines.join("\n")
        }
        Err(_) => summary.chars().take(1500).collect(),
    }
}

pub async fn generate(
    State(state): State<AppState>,
    Extension(auth): Extension<CurrentUser>,
    Json(body): Json<WeekBody>,
) -> ApiResult<Json<Value>> {
    let user = current_user(&state.db, &auth).await?;
    require_manager(&user)?;
    let offset = app_offset(&state.db).await;
    let week = week_from(body.week_start.as_deref(), &offset)?;
    let end = week + Duration::days(6);
    ensure_week(&state.db, week).await?;

    let employees = sqlx::query_as::<_, Employee>(&format!(
        "SELECT {} FROM employees ORDER BY (role = 'Руководитель') DESC, name, id",
        EMPLOYEE_COLS
    ))
    .fetch_all(&state.db)
    .await?;
    let meetings: Vec<(i64, String, String)> = sqlx::query_as(
        "SELECT m.id, m.title, s.summary_text
         FROM meetings m JOIN summaries s ON s.meeting_id = m.id
         ORDER BY m.created_at DESC, s.id DESC LIMIT 3",
    )
    .fetch_all(&state.db)
    .await?;

    // Текущий (утверждённый) график недели — без прежнего предложения AI.
    let current = sqlx::query_as::<_, ShiftRow>(&format!(
        "SELECT {} FROM shifts WHERE shift_date BETWEEN $1 AND $2 ORDER BY shift_date, employee_id",
        SHIFT_COLS
    ))
    .bind(week - Duration::days(1))
    .bind(end)
    .fetch_all(&state.db)
    .await?;

    let mut people_lines = String::new();
    for e in &employees {
        people_lines.push_str(&format!(
            "- id={} · {} · {} · отдел: {} · роль: {}\n",
            e.id,
            e.name,
            e.position,
            if e.department.is_empty() { "—" } else { e.department.as_str() },
            e.role
        ));
    }
    let mut shift_lines = String::new();
    for s in current.iter().filter(|s| s.shift_date >= week) {
        // прежнее предложение AI будет заменено, поэтому показываем утверждённое время
        let time = if s.status == "ai" {
            if s.was().is_empty() {
                continue;
            }
            s.was()
        } else {
            s.time()
        };
        shift_lines.push_str(&format!("- сотрудник {} · {} · {}\n", s.employee_id, iso(s.shift_date), time));
    }
    if shift_lines.is_empty() {
        shift_lines.push_str("(смен на эту неделю пока нет)\n");
    }
    let mut meeting_lines = String::new();
    for (id, title, summary) in &meetings {
        meeting_lines.push_str(&format!("Встреча id={} «{}»:\n{}\n\n", id, title, protocol_text(summary)));
    }
    if meeting_lines.is_empty() {
        meeting_lines.push_str("(протоколов встреч пока нет)\n");
    }

    let prompt = format!(
        r#"Ты — планировщик смен типографии «Фотопринт». Предложи ИЗМЕНЕНИЯ графика на неделю с {week} по {end}.

ПРАВИЛА:
1. Предлагай изменения только если они прямо следуют из протоколов встреч (например, нужно усилить вечернюю смену из-за роста заказов). Ничего не выдумывай.
2. Если изменений не требуется, верни пустой массив "changes".
3. Используй только employeeId из списка сотрудников и только даты внутри недели ({week} … {end}), формат ГГГГ-ММ-ДД.
4. Время начала и конца — в формате ЧЧ:ММ. Между сменами одного сотрудника должно быть не менее 12 часов.
5. Не больше {max} изменений. Руководителей не трогай, если об этом прямо не сказано.
6. "summary" — одна короткая фраза (до 100 символов) в стиле «AI предлагает усилить вечернюю смену в чт и пт».
7. "why" — 1–3 предложения: почему такой график, со ссылкой на факты из протокола.
8. "sourceMeetingId" — id встречи, из протокола которой взято основание (или не указывай).
9. Язык — русский.

Сотрудники:
{people}
Текущий график недели:
{shifts}
Протоколы последних встреч:
{meetings}
Верни СТРОГО JSON."#,
        week = iso(week),
        end = iso(end),
        max = MAX_AI_CHANGES,
        people = people_lines,
        shifts = shift_lines,
        meetings = meeting_lines,
    );

    let schema = json!({
        "type": "object",
        "properties": {
            "summary": { "type": "string" },
            "why": { "type": "string" },
            "sourceMeetingId": { "type": "integer" },
            "changes": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "employeeId": { "type": "integer" },
                        "date": { "type": "string" },
                        "start": { "type": "string" },
                        "end": { "type": "string" }
                    },
                    "required": ["employeeId", "date", "start", "end"]
                }
            }
        },
        "required": ["summary", "why", "changes"]
    });

    let raw = crate::ai::generate_json(prompt, schema)
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, format!("Ассистент недоступен: {}", e)))?;
    let plan: AiPlan = serde_json::from_value(raw)
        .map_err(|_| ApiError::new(StatusCode::BAD_GATEWAY, "Ассистент вернул ответ неверного формата"))?;

    // --- проверка и применение предложений в одной транзакции ---
    let valid_employees: BTreeSet<i64> = employees.iter().map(|e| e.id).collect();
    let mut tx = state.db.begin().await?;
    revert_proposal(&mut tx, week).await?;

    let existing = sqlx::query_as::<_, ShiftRow>(&format!(
        "SELECT {} FROM shifts WHERE shift_date BETWEEN $1 AND $2",
        SHIFT_COLS
    ))
    .bind(week - Duration::days(1))
    .bind(end + Duration::days(1))
    .fetch_all(&mut *tx)
    .await?;
    // (сотрудник, дата) -> (id смены, начало, конец)
    let mut map: HashMap<(i64, NaiveDate), (i64, i32, i32)> = existing
        .iter()
        .map(|s| ((s.employee_id, s.shift_date), (s.id, s.start_min, s.end_min)))
        .collect();

    let mut applied = 0usize;
    for change in plan.changes.iter().take(MAX_AI_CHANGES) {
        let employee = match change.employee_id.as_i64() {
            Some(id) if valid_employees.contains(&id) => id,
            _ => continue,
        };
        let date = match NaiveDate::parse_from_str(change.date.trim(), "%Y-%m-%d") {
            Ok(d) if d >= week && d <= end => d,
            _ => continue,
        };
        let (start, finish) = match util::parse_time_range(&format!("{}-{}", change.start.trim(), change.end.trim())) {
            Some(range) => range,
            None => continue,
        };
        let current_shift = map.get(&(employee, date)).copied();
        if let Some((_, s, e)) = current_shift {
            if s == start && e == finish {
                continue;
            }
        }
        // отдых до и после смены не меньше 12 часов
        let prev_ok = match map.get(&(employee, date - Duration::days(1))) {
            Some((_, ps, pe)) => util::rest_minutes(*ps, *pe, start) >= MIN_REST_MINUTES,
            None => true,
        };
        let next_ok = match map.get(&(employee, date + Duration::days(1))) {
            Some((_, ns, _)) => util::rest_minutes(start, finish, *ns) >= MIN_REST_MINUTES,
            None => true,
        };
        if !prev_ok || !next_ok {
            continue;
        }

        match current_shift {
            Some((shift_id, old_start, old_end)) => {
                sqlx::query(
                    "UPDATE shifts SET prev_start_min = $2, prev_end_min = $3, start_min = $4, end_min = $5,
                            status = 'ai', updated_at = NOW() WHERE id = $1",
                )
                .bind(shift_id)
                .bind(old_start)
                .bind(old_end)
                .bind(start)
                .bind(finish)
                .execute(&mut *tx)
                .await?;
                map.insert((employee, date), (shift_id, start, finish));
            }
            None => {
                let (new_id,): (i64,) = sqlx::query_as(
                    "INSERT INTO shifts (employee_id, shift_date, start_min, end_min, status)
                     VALUES ($1, $2, $3, $4, 'ai') RETURNING id",
                )
                .bind(employee)
                .bind(date)
                .bind(start)
                .bind(finish)
                .fetch_one(&mut *tx)
                .await?;
                map.insert((employee, date), (new_id, start, finish));
            }
        }
        applied += 1;
    }

    if applied == 0 {
        sqlx::query(
            "UPDATE schedule_weeks SET proposal_text = NULL, why_text = NULL, why_source = NULL,
                    why_meeting_id = NULL, updated_at = NOW() WHERE week_start = $1",
        )
        .bind(week)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        mark_kind_read(&state.db, "schedule_proposal", &iso(week)).await;
        return Ok(Json(json!({ "proposal": Value::Null })));
    }

    let source = plan
        .source_meeting_id
        .as_ref()
        .and_then(|m| m.as_i64())
        .and_then(|id| meetings.iter().find(|m| m.0 == id));
    let summary: String = plan.summary.trim().chars().take(200).collect();
    let why: String = plan.why.trim().chars().take(1000).collect();
    let why = if why.is_empty() {
        "Изменения предложены на основе протоколов последних встреч.".to_string()
    } else {
        why
    };
    sqlx::query(
        "UPDATE schedule_weeks SET published = FALSE, proposal_text = $2, why_text = $3, why_source = $4,
                why_meeting_id = $5, updated_at = NOW() WHERE week_start = $1",
    )
    .bind(week)
    .bind(if summary.is_empty() { None } else { Some(summary) })
    .bind(&why)
    .bind(source.map(|m| m.1.clone()))
    .bind(source.map(|m| m.0))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    let proposal = proposal_for_week(&state.db, week).await?;
    mark_kind_read(&state.db, "schedule_proposal", &iso(week)).await;
    let people_text = proposal["changes"]
        .as_array()
        .map(|list| {
            let mut names: Vec<String> = Vec::new();
            for c in list {
                if let Some(n) = c["who"].as_str() {
                    if !names.iter().any(|x| x == n) {
                        names.push(n.to_string());
                    }
                }
            }
            names.join(", ")
        })
        .unwrap_or_default();
    let notice = NewNotice::new(
        "График смен ждёт решения",
        format!("AI предлагает изменить {} {}: {}.", applied, util::plural(applied as i64, "смену", "смены", "смен"), people_text),
        "График",
        "Посмотреть изменения",
        "schedule.html",
        true,
    )
    .with_ref("schedule_proposal", &iso(week));
    log_notify_error(notify_managers(&state.db, None, &notice).await);

    Ok(Json(json!({ "proposal": proposal })))
}

pub async fn reject_proposal(
    State(state): State<AppState>,
    Extension(auth): Extension<CurrentUser>,
    Json(body): Json<WeekBody>,
) -> ApiResult<Json<Value>> {
    let user = current_user(&state.db, &auth).await?;
    require_manager(&user)?;
    let offset = app_offset(&state.db).await;
    let week = week_from(body.week_start.as_deref(), &offset)?;

    let mut tx = state.db.begin().await?;
    revert_proposal(&mut tx, week).await?;
    sqlx::query(
        "UPDATE schedule_weeks SET proposal_text = NULL, why_text = NULL, why_source = NULL,
                why_meeting_id = NULL, updated_at = NOW() WHERE week_start = $1",
    )
    .bind(week)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    mark_kind_read(&state.db, "schedule_proposal", &iso(week)).await;
    Ok(Json(json!({ "ok": true })))
}

pub async fn publish(
    State(state): State<AppState>,
    Extension(auth): Extension<CurrentUser>,
    Json(body): Json<WeekBody>,
) -> ApiResult<Json<Value>> {
    let user = current_user(&state.db, &auth).await?;
    require_manager(&user)?;
    let offset = app_offset(&state.db).await;
    let week = week_from(body.week_start.as_deref(), &offset)?;
    let end = week + Duration::days(6);
    ensure_week(&state.db, week).await?;

    let mut tx = state.db.begin().await?;
    let changed = sqlx::query_as::<_, ShiftRow>(&format!(
        "SELECT {} FROM shifts WHERE status = 'ai' AND shift_date BETWEEN $1 AND $2 ORDER BY shift_date",
        SHIFT_COLS
    ))
    .bind(week)
    .bind(end)
    .fetch_all(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE shifts SET status = 'confirmed', prev_start_min = NULL, prev_end_min = NULL, updated_at = NOW()
         WHERE status = 'ai' AND shift_date BETWEEN $1 AND $2",
    )
    .bind(week)
    .bind(end)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE schedule_weeks SET published = TRUE, published_at = NOW(), proposal_text = NULL, updated_at = NOW()
         WHERE week_start = $1",
    )
    .bind(week)
    .execute(&mut *tx)
    .await?;
    let everyone: Vec<i64> = sqlx::query_scalar(
        "SELECT DISTINCT employee_id FROM shifts WHERE shift_date BETWEEN $1 AND $2",
    )
    .bind(week)
    .bind(end)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;

    mark_kind_read(&state.db, "schedule_proposal", &iso(week)).await;

    // Сотрудники получают уведомление только после публикации.
    if changed.is_empty() {
        for employee in everyone {
            if employee == user.id {
                continue;
            }
            let notice = NewNotice::new(
                "Опубликован график",
                format!("График на неделю с {} по {} утверждён.", long_date(week), long_date(end)),
                "График",
                "Открыть график",
                "schedule.html",
                false,
            );
            log_notify_error(notify_one(&state.db, employee, &notice).await);
        }
    } else {
        let mut per_employee: BTreeMap<i64, Vec<String>> = BTreeMap::new();
        for s in &changed {
            let line = if s.was().is_empty() {
                format!("{} — {}", long_date(s.shift_date), s.time())
            } else {
                format!("{} — {} (было {})", long_date(s.shift_date), s.time(), s.was())
            };
            per_employee.entry(s.employee_id).or_default().push(line);
        }
        for (employee, lines) in per_employee {
            if employee == user.id {
                continue;
            }
            let notice = NewNotice::new(
                "Опубликован новый график",
                format!("Ваши смены изменились: {}", lines.join("; ")),
                "График",
                "Открыть график",
                "schedule.html",
                false,
            );
            log_notify_error(notify_one(&state.db, employee, &notice).await);
        }
    }
    Ok(Json(json!({ "published": true })))
}
