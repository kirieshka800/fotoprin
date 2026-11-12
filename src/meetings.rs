// Бэкенд для двух частей приложения:
//   1) вкладка «Обзор»: /api/me, /api/stats, /api/activity, /api/settings
//   2) встречи: создание, список, карточка, статус обработки, вопросы по встрече, экспорт, удаление
// Плюс справочники для формы создания встречи: /api/employees и /api/departments.

use axum::{
    extract::{Multipart, Path, Query, State},
    http::StatusCode,
    Json,
};
use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{postgres::PgRow, PgPool, Postgres, QueryBuilder, Row};
use std::io::Read;
use std::time::Duration;

use crate::{
    analyze_meeting_with_ai, create_error_response, ollama_generate, ollama_model_name, AppState,
    ProtocolResult, ResponseError,
};

// Больше этого количества символов в нейросеть не отправляем (контекст qwen 7b ограничен)
const MAX_TEXT_FOR_MODEL: usize = 14000;

// Папка, куда сохраняются аудиозаписи (отдаются браузеру по адресу /uploads/...)
pub const UPLOADS_DIR: &str = "uploads";

const AUDIO_EXTENSIONS: [&str; 11] = [
    "mp3", "wav", "m4a", "ogg", "oga", "opus", "webm", "aac", "flac", "mp4", "mov",
];

fn file_extension(file_name: &str) -> String {
    file_name.rsplit('.').next().unwrap_or("").to_lowercase()
}

fn is_audio_file(file_name: &str) -> bool {
    AUDIO_EXTENSIONS.contains(&file_extension(file_name).as_str())
}

fn db_error(err: sqlx::Error) -> ResponseError {
    create_error_response(
        StatusCode::INTERNAL_SERVER_ERROR,
        &format!("Ошибка БД: {}", err),
    )
}

fn bad_request(message: &str) -> ResponseError {
    create_error_response(StatusCode::BAD_REQUEST, message)
}

// ============================================================
//  Запуск: незавершённые встречи после перезапуска сервера
// ============================================================

pub async fn mark_stuck_meetings(db: &PgPool) {
    let _ = sqlx::query(
        "UPDATE meetings SET status = 'error', \
         error_message = 'Сервер был перезапущен во время обработки. Создайте встречу заново.' \
         WHERE status = 'processing'",
    )
    .execute(db)
    .await;
}

// ============================================================
//  Обзор: профиль, настройки, статистика, лента
// ============================================================

fn default_settings() -> Value {
    json!({
        "company": "Фотопринт",
        "timezone": "Москва · UTC+3",
        "autonomous": true,
        "createTasks": true,
        "sendReminders": true,
        "publishRule": "Только после подтверждения",
        "telegram": { "connected": false, "enabled": false },
        "email": { "connected": false, "enabled": false, "address": "" },
        "quietHours": { "enabled": true, "from": "20:00", "to": "09:00" }
    })
}

async fn load_settings(db: &PgPool) -> Result<Value, ResponseError> {
    let row: Option<(Value,)> = sqlx::query_as("SELECT data FROM app_settings WHERE id = 1")
        .fetch_optional(db)
        .await
        .map_err(db_error)?;
    let mut settings = default_settings();
    if let Some((saved,)) = row {
        if let (Some(target), Some(source)) = (settings.as_object_mut(), saved.as_object()) {
            for (key, value) in source {
                target.insert(key.clone(), value.clone());
            }
        }
    }
    Ok(settings)
}

// GET /api/settings
pub async fn get_settings(State(state): State<AppState>) -> Result<Json<Value>, ResponseError> {
    Ok(Json(load_settings(&state.db).await?))
}

// PUT /api/settings  (принимает весь объект настроек целиком)
pub async fn put_settings(
    State(state): State<AppState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ResponseError> {
    if !body.is_object() {
        return Err(bad_request("Настройки должны быть JSON-объектом"));
    }
    sqlx::query(
        "INSERT INTO app_settings (id, data) VALUES (1, $1) \
         ON CONFLICT (id) DO UPDATE SET data = EXCLUDED.data",
    )
    .bind(&body)
    .execute(&state.db)
    .await
    .map_err(db_error)?;
    Ok(Json(load_settings(&state.db).await?))
}

// GET /api/stats
pub async fn get_stats(State(state): State<AppState>) -> Result<Json<Value>, ResponseError> {
    let db = &state.db;

    let meetings_week: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM meetings WHERE meeting_date >= NOW() - INTERVAL '7 days' \
         AND meeting_date <= NOW() + INTERVAL '1 day'",
    )
    .fetch_one(db)
    .await
    .map_err(db_error)?;

    let meetings_prev_week: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM meetings WHERE meeting_date >= NOW() - INTERVAL '14 days' \
         AND meeting_date < NOW() - INTERVAL '7 days'",
    )
    .fetch_one(db)
    .await
    .map_err(db_error)?;

    let tasks_active: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE status <> 'done'")
            .fetch_one(db)
            .await
            .map_err(db_error)?;

    // Требуют внимания: нет ответственного или срока («требует уточнения»)
    let tasks_attention: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE status <> 'done' AND \
         (COALESCE(assignee, '') ILIKE '%требует уточнения%' OR due_text ILIKE '%требует уточнения%' \
          OR COALESCE(assignee, '') = '')",
    )
    .fetch_one(db)
    .await
    .map_err(db_error)?;

    // Оценка сэкономленного времени: ~30 минут на каждую обработанную встречу недели
    // и ~6 минут на каждую созданную из встреч задачу.
    let ready_week: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM meetings WHERE status = 'ready' \
         AND meeting_date >= NOW() - INTERVAL '7 days' AND meeting_date <= NOW() + INTERVAL '1 day'",
    )
    .fetch_one(db)
    .await
    .map_err(db_error)?;
    let tasks_week: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tasks WHERE created_at >= NOW() - INTERVAL '7 days'",
    )
    .fetch_one(db)
    .await
    .map_err(db_error)?;
    let saved_hours = ((ready_week as f64 * 0.5 + tasks_week as f64 * 0.1) * 10.0).round() / 10.0;

    Ok(Json(json!({
        "meetingsWeek": meetings_week,
        "meetingsDelta": meetings_week - meetings_prev_week,
        "tasksActive": tasks_active,
        "tasksAttention": tasks_attention,
        "savedHours": saved_hours
    })))
}

// GET /api/activity  — лента «Пока вы были заняты»
pub async fn get_activity(State(state): State<AppState>) -> Result<Json<Value>, ResponseError> {
    let rows = sqlx::query(
        "SELECT id, kind, title, text, created_at FROM activity ORDER BY created_at DESC, id DESC LIMIT 10",
    )
    .fetch_all(&state.db)
    .await
    .map_err(db_error)?;

    let mut items = Vec::new();
    for row in rows {
        let created: DateTime<Utc> = row.get("created_at");
        items.push(json!({
            "id": row.get::<i64, _>("id"),
            "kind": row.get::<String, _>("kind"),
            "title": row.get::<String, _>("title"),
            "text": row.get::<String, _>("text"),
            "at": created.to_rfc3339()
        }));
    }
    Ok(Json(Value::Array(items)))
}

async fn add_activity(db: &PgPool, kind: &str, title: &str, text: &str, meeting_id: i64) {
    let _ = sqlx::query(
        "INSERT INTO activity (kind, title, text, meeting_id) VALUES ($1, $2, $3, $4)",
    )
    .bind(kind)
    .bind(title)
    .bind(text)
    .bind(meeting_id)
    .execute(db)
    .await;
}

// ============================================================
//  Справочники: сотрудники и отделы
// ============================================================

// GET /api/employees
pub async fn list_employees(State(state): State<AppState>) -> Result<Json<Value>, ResponseError> {
    let rows = sqlx::query("SELECT id, name, position, department, role FROM employees ORDER BY name")
        .fetch_all(&state.db)
        .await
        .map_err(db_error)?;
    let mut items = Vec::new();
    for row in rows {
        items.push(json!({
            "id": row.get::<i64, _>("id"),
            "name": row.get::<String, _>("name"),
            "position": row.get::<String, _>("position"),
            "department": row.get::<String, _>("department"),
            "role": row.get::<String, _>("role")
        }));
    }
    Ok(Json(Value::Array(items)))
}

#[derive(Deserialize)]
pub struct NewEmployee {
    pub name: String,
    #[serde(default)]
    pub position: String,
    #[serde(default)]
    pub department: String,
    #[serde(default)]
    pub role: String,
}

// POST /api/employees { name, position, department, role }
pub async fn create_employee(
    State(state): State<AppState>,
    Json(body): Json<NewEmployee>,
) -> Result<(StatusCode, Json<Value>), ResponseError> {
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return Err(bad_request("Укажите имя сотрудника"));
    }
    let role = if body.role.trim().is_empty() {
        "Сотрудник".to_string()
    } else {
        body.role.trim().to_string()
    };
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO employees (name, position, department, role) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(&name)
    .bind(body.position.trim())
    .bind(body.department.trim())
    .bind(&role)
    .fetch_one(&state.db)
    .await
    .map_err(db_error)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": id, "name": name, "position": body.position.trim(),
            "department": body.department.trim(), "role": role
        })),
    ))
}

// GET /api/departments  -> ["Производство", ...]
pub async fn list_departments(State(state): State<AppState>) -> Result<Json<Value>, ResponseError> {
    let names: Vec<String> = sqlx::query_scalar("SELECT name FROM departments ORDER BY name")
        .fetch_all(&state.db)
        .await
        .map_err(db_error)?;
    Ok(Json(json!(names)))
}

// ============================================================
//  Встречи: чтение
// ============================================================

const MEETING_SELECT: &str = "SELECT m.id, m.title, m.department, m.meeting_date, m.duration_min, \
    m.status, m.progress, m.description, m.error_message, m.source_name, \
    m.audio_file, m.audio_duration, m.transcript, \
    COALESCE((SELECT array_agg(p.employee_id ORDER BY p.employee_id) \
              FROM meeting_participants p WHERE p.meeting_id = m.id), ARRAY[]::BIGINT[]) AS participant_ids, \
    (SELECT COUNT(*) FROM tasks t WHERE t.meeting_id = m.id) AS tasks_count, \
    (SELECT s.summary_text FROM summaries s WHERE s.meeting_id = m.id ORDER BY s.id DESC LIMIT 1) AS summary_text \
    FROM meetings m";

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

// Превращаем сохранённый протокол (JSON от нейросети) в формат, который ждёт страница встречи
fn summary_to_value(summary_text: &str) -> Value {
    let protocol: ProtocolResult = match serde_json::from_str(summary_text) {
        Ok(parsed) => parsed,
        Err(_) => return Value::Null,
    };

    let mut decisions: Vec<String> = Vec::new();
    let mut topics: Vec<Value> = Vec::new();
    for item in &protocol.items {
        if item.category == "решено" {
            decisions.push(item.text.clone());
        } else {
            topics.push(json!({
                "title": capitalize(&item.category),
                "text": item.text,
                "time": ""
            }));
        }
    }

    let mut highlight = protocol.highlight.clone();
    if highlight.trim().is_empty() {
        if let Some(first) = decisions.first() {
            highlight = first.clone();
        }
    }

    json!({
        "highlight": highlight,
        "topics": topics,
        "decisions": decisions,
        "source": format!("текст встречи, обработано моделью {}.", ollama_model_name())
    })
}

fn row_to_meeting(row: &PgRow) -> Value {
    let date: DateTime<Utc> = row.get("meeting_date");
    let summary_text: Option<String> = row.get("summary_text");
    let audio_file: Option<String> = row.get("audio_file");
    let summary = match summary_text {
        Some(text) => summary_to_value(&text),
        None => Value::Null,
    };
    json!({
        "id": row.get::<i64, _>("id"),
        "title": row.get::<String, _>("title"),
        "department": row.get::<String, _>("department"),
        "date": date.to_rfc3339(),
        "durationMin": row.get::<i32, _>("duration_min"),
        "participantIds": row.get::<Vec<i64>, _>("participant_ids"),
        "status": row.get::<String, _>("status"),
        "progress": row.get::<i32, _>("progress"),
        "tasksCount": row.get::<i64, _>("tasks_count"),
        "description": row.get::<String, _>("description"),
        "errorMessage": row.get::<Option<String>, _>("error_message"),
        "sourceName": row.get::<Option<String>, _>("source_name"),
        "summary": summary,
        "transcript": row.get::<Value, _>("transcript"),
        "audioUrl": match audio_file {
            Some(name) => format!("/uploads/{}", name),
            None => String::new(),
        },
        "audioDuration": row.get::<i32, _>("audio_duration")
    })
}

#[derive(Deserialize)]
pub struct MeetingFilters {
    pub search: Option<String>,
    pub department: Option<String>,
    pub period: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
}

fn apply_filters(builder: &mut QueryBuilder<Postgres>, filters: &MeetingFilters) {
    builder.push(" WHERE TRUE");
    if let Some(search) = &filters.search {
        let search = search.trim();
        if !search.is_empty() {
            let pattern = format!("%{}%", search);
            builder.push(" AND (m.title ILIKE ");
            builder.push_bind(pattern.clone());
            builder.push(" OR m.description ILIKE ");
            builder.push_bind(pattern);
            builder.push(")");
        }
    }
    if let Some(department) = &filters.department {
        if !department.is_empty() && department != "all" && department != "Все отделы" {
            builder.push(" AND m.department = ");
            builder.push_bind(department.clone());
        }
    }
    if let Some(period) = &filters.period {
        if period == "week" {
            builder.push(" AND m.meeting_date >= NOW() - INTERVAL '7 days'");
        } else if period == "month" {
            builder.push(" AND m.meeting_date >= NOW() - INTERVAL '30 days'");
        }
    }
    if let Some(status) = &filters.status {
        if !status.is_empty() && status != "all" {
            builder.push(" AND m.status = ");
            builder.push_bind(status.clone());
        }
    }
}

// GET /api/meetings?search&department&period(week|month|all)&status&limit -> { items, total }
pub async fn list_meetings(
    State(state): State<AppState>,
    Query(filters): Query<MeetingFilters>,
) -> Result<Json<Value>, ResponseError> {
    let mut count_builder: QueryBuilder<Postgres> =
        QueryBuilder::new("SELECT COUNT(*) FROM meetings m");
    apply_filters(&mut count_builder, &filters);
    let total: i64 = count_builder
        .build_query_scalar()
        .fetch_one(&state.db)
        .await
        .map_err(db_error)?;

    let mut builder: QueryBuilder<Postgres> = QueryBuilder::new(MEETING_SELECT);
    apply_filters(&mut builder, &filters);
    builder.push(" ORDER BY m.meeting_date DESC, m.id DESC");
    if let Some(limit) = filters.limit {
        if limit > 0 {
            builder.push(" LIMIT ");
            builder.push_bind(limit);
        }
    }
    let rows = builder
        .build()
        .fetch_all(&state.db)
        .await
        .map_err(db_error)?;

    let items: Vec<Value> = rows.iter().map(row_to_meeting).collect();
    Ok(Json(json!({ "items": items, "total": total })))
}

async fn load_meeting(db: &PgPool, id: i64) -> Result<Value, ResponseError> {
    let sql = format!("{} WHERE m.id = $1", MEETING_SELECT);
    let row = sqlx::query(&sql)
        .bind(id)
        .fetch_optional(db)
        .await
        .map_err(db_error)?
        .ok_or_else(|| create_error_response(StatusCode::NOT_FOUND, "Встреча не найдена"))?;
    Ok(row_to_meeting(&row))
}

// GET /api/meetings/:id
pub async fn get_meeting(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ResponseError> {
    let mut meeting = load_meeting(&state.db, id).await?;

    let task_rows = sqlx::query(
        "SELECT id, title, description, assignee, due_text, status FROM tasks \
         WHERE meeting_id = $1 ORDER BY id",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await
    .map_err(db_error)?;

    let mut tasks = Vec::new();
    for row in task_rows {
        let title: String = row.get("title");
        let description: String = row.get("description");
        tasks.push(json!({
            "id": row.get::<i64, _>("id"),
            "title": if title.is_empty() { description } else { title },
            "assignee": row.get::<Option<String>, _>("assignee").unwrap_or_default(),
            "dueText": row.get::<String, _>("due_text"),
            "status": row.get::<String, _>("status")
        }));
    }
    meeting["tasks"] = Value::Array(tasks);
    Ok(Json(meeting))
}

// GET /api/meetings/:id/status -> { status, progress, error }
pub async fn meeting_status(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ResponseError> {
    let row: Option<(String, i32, Option<String>)> =
        sqlx::query_as("SELECT status, progress, error_message FROM meetings WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await
            .map_err(db_error)?;
    match row {
        Some((status, progress, error)) => Ok(Json(json!({
            "status": status, "progress": progress, "error": error
        }))),
        None => Err(create_error_response(StatusCode::NOT_FOUND, "Встреча не найдена")),
    }
}

// DELETE /api/meetings/:id
pub async fn delete_meeting(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ResponseError> {
    let audio_file: Option<Option<String>> =
        sqlx::query_scalar("SELECT audio_file FROM meetings WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await
            .map_err(db_error)?;
    if let Some(Some(name)) = audio_file {
        let _ = std::fs::remove_file(format!("{}/{}", UPLOADS_DIR, name));
    }
    let result = sqlx::query("DELETE FROM meetings WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(db_error)?;
    if result.rows_affected() == 0 {
        return Err(create_error_response(StatusCode::NOT_FOUND, "Встреча не найдена"));
    }
    Ok(Json(json!({ "ok": true })))
}

// ============================================================
//  Встречи: создание
// ============================================================

fn parse_meeting_date(text: &str) -> DateTime<Utc> {
    let text = text.trim();
    if let Ok(parsed) = DateTime::parse_from_rfc3339(text) {
        return parsed.with_timezone(&Utc);
    }
    let formats = ["%Y-%m-%dT%H:%M", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M", "%Y-%m-%d %H:%M:%S"];
    for format in formats {
        if let Ok(parsed) = NaiveDateTime::parse_from_str(text, format) {
            return Utc.from_utc_datetime(&parsed);
        }
    }
    if let Ok(day) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        // Если выбрана сегодняшняя дата без времени — берём текущий момент
        if day == Utc::now().date_naive() {
            return Utc::now();
        }
        if let Some(moment) = day.and_hms_opt(10, 0, 0) {
            return Utc.from_utc_datetime(&moment);
        }
    }
    Utc::now()
}

// participantIds приходят JSON-строкой: [1,2] или ["1","2"]
fn parse_participant_ids(text: &str) -> Vec<i64> {
    let parsed: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let mut ids = Vec::new();
    if let Some(list) = parsed.as_array() {
        for item in list {
            if let Some(number) = item.as_i64() {
                ids.push(number);
            } else if let Some(text) = item.as_str() {
                if let Ok(number) = text.trim().parse::<i64>() {
                    ids.push(number);
                }
            }
        }
    }
    ids
}

fn action_enabled(actions: &Value, name: &str) -> bool {
    // по умолчанию включено, как и в форме
    actions.get(name).and_then(|v| v.as_bool()).unwrap_or(true)
}

// POST /api/meetings  (multipart: title, date, department, notes, participantIds, actions, file)
pub async fn create_meeting(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), ResponseError> {
    let mut title = String::new();
    let mut date_text = String::new();
    let mut department = String::new();
    let mut notes = String::new();
    let mut participants_text = String::new();
    let mut actions_text = String::new();
    let mut file_name: Option<String> = None;
    let mut file_bytes: Vec<u8> = Vec::new();

    loop {
        let field = multipart
            .next_field()
            .await
            .map_err(|e| bad_request(&format!("Не удалось прочитать форму: {}", e)))?;
        let field = match field {
            Some(field) => field,
            None => break,
        };
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            let original_name = field.file_name().unwrap_or("").to_string();
            let bytes = field
                .bytes()
                .await
                .map_err(|e| bad_request(&format!("Не удалось прочитать файл: {}", e)))?;
            if !bytes.is_empty() {
                file_name = Some(original_name);
                file_bytes = bytes.to_vec();
            }
        } else {
            let value = field
                .text()
                .await
                .map_err(|e| bad_request(&format!("Не удалось прочитать поле {}: {}", name, e)))?;
            match name.as_str() {
                "title" => title = value,
                "date" => date_text = value,
                "department" => department = value,
                "notes" => notes = value,
                "participantIds" => participants_text = value,
                "actions" => actions_text = value,
                _ => {}
            }
        }
    }

    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(bad_request("Укажите название встречи"));
    }
    if file_name.is_none() && notes.trim().is_empty() {
        return Err(bad_request(
            "Загрузите файл со стенограммой или введите текст встречи в поле заметок",
        ));
    }

    let meeting_date = parse_meeting_date(&date_text);
    let participant_ids = parse_participant_ids(&participants_text);
    let actions: Value = serde_json::from_str(&actions_text).unwrap_or(json!({}));

    let meeting_id: i64 = sqlx::query_scalar(
        "INSERT INTO meetings (title, raw_text, department, meeting_date, status, progress, \
         description, source_name, actions) \
         VALUES ($1, $2, $3, $4, 'processing', 5, $5, $6, $7) RETURNING id",
    )
    .bind(&title)
    .bind(notes.trim())
    .bind(department.trim())
    .bind(meeting_date)
    .bind(notes.trim())
    .bind(&file_name)
    .bind(&actions)
    .fetch_one(&state.db)
    .await
    .map_err(db_error)?;

    // Аудио (mp3, голосовые и т.п.) сохраняем на диск, чтобы его можно было слушать на странице встречи
    if let Some(name) = &file_name {
        if is_audio_file(name) {
            let stored_name = format!("meeting-{}.{}", meeting_id, file_extension(name));
            let write_result = std::fs::create_dir_all(UPLOADS_DIR)
                .and_then(|_| std::fs::write(format!("{}/{}", UPLOADS_DIR, stored_name), &file_bytes));
            if let Err(err) = write_result {
                fail_meeting(&state.db, meeting_id, &format!("Не удалось сохранить запись на диск: {}", err)).await;
            } else {
                let _ = sqlx::query("UPDATE meetings SET audio_file = $2 WHERE id = $1")
                    .bind(meeting_id)
                    .bind(&stored_name)
                    .execute(&state.db)
                    .await;
            }
        }
    }

    for employee_id in &participant_ids {
        // несуществующие сотрудники просто пропускаются
        let _ = sqlx::query(
            "INSERT INTO meeting_participants (meeting_id, employee_id) \
             SELECT $1, id FROM employees WHERE id = $2 ON CONFLICT DO NOTHING",
        )
        .bind(meeting_id)
        .bind(employee_id)
        .execute(&state.db)
        .await;
    }

    // Тяжёлая работа (чтение файла + нейросеть) идёт в фоне, страница опрашивает /status
    let create_tasks = action_enabled(&actions, "tasks");
    let db = state.db.clone();
    tokio::spawn(async move {
        process_meeting(db, meeting_id, title, notes, file_name, file_bytes, create_tasks).await;
    });

    let meeting = load_meeting(&state.db, meeting_id).await?;
    Ok((StatusCode::CREATED, Json(meeting)))
}

async fn set_progress(db: &PgPool, meeting_id: i64, progress: i32) {
    let _ = sqlx::query("UPDATE meetings SET progress = $2 WHERE id = $1 AND status = 'processing'")
        .bind(meeting_id)
        .bind(progress)
        .execute(db)
        .await;
}

async fn fail_meeting(db: &PgPool, meeting_id: i64, message: &str) {
    let _ = sqlx::query(
        "UPDATE meetings SET status = 'error', error_message = $2 WHERE id = $1",
    )
    .bind(meeting_id)
    .bind(message)
    .execute(db)
    .await;
}

pub(crate) async fn process_meeting(
    db: PgPool,
    meeting_id: i64,
    title: String,
    notes: String,
    file_name: Option<String>,
    file_bytes: Vec<u8>,
    create_tasks: bool,
) {
    // 1. Достаём текст из файла
    let mut full_text = notes.trim().to_string();
    let audio_name = match &file_name {
        Some(name) if is_audio_file(name) => Some(name.clone()),
        _ => None,
    };
    if let Some(name) = &audio_name {
        // Аудио: переводим речь в текст (Whisper), расшифровку сохраняем отдельно
        match transcribe_audio(name, file_bytes).await {
            Ok(transcript) => {
                let rows = transcript_to_json(&transcript.segments);
                let _ = sqlx::query(
                    "UPDATE meetings SET transcript = $2, audio_duration = $3, progress = 20 WHERE id = $1",
                )
                .bind(meeting_id)
                .bind(&rows)
                .bind(transcript.duration_seconds)
                .execute(&db)
                .await;
                if !full_text.is_empty() {
                    full_text.push_str("\n\n");
                }
                full_text.push_str(&transcript.text);
            }
            Err(message) => {
                if full_text.is_empty() {
                    fail_meeting(&db, meeting_id, &message).await;
                    return;
                }
                // есть заметки — продолжаем только по ним, запись остаётся доступной для прослушивания
            }
        }
    } else if let Some(name) = &file_name {
        let name_for_job = name.clone();
        let extracted =
            tokio::task::spawn_blocking(move || extract_text(&name_for_job, &file_bytes)).await;
        match extracted {
            Ok(Ok(text)) => {
                if !full_text.is_empty() {
                    full_text.push_str("\n\n");
                }
                full_text.push_str(&text);
            }
            Ok(Err(message)) => {
                fail_meeting(&db, meeting_id, &message).await;
                return;
            }
            Err(_) => {
                fail_meeting(&db, meeting_id, "Не удалось прочитать файл (повреждён или не поддерживается)").await;
                return;
            }
        }
    }
    let full_text = full_text.trim().to_string();
    if full_text.is_empty() {
        fail_meeting(&db, meeting_id, "В файле не нашлось текста").await;
        return;
    }
    let _ = sqlx::query("UPDATE meetings SET raw_text = $2, progress = 25 WHERE id = $1")
        .bind(meeting_id)
        .bind(&full_text)
        .execute(&db)
        .await;

    // 2. Нейросеть. Пока она думает, постепенно двигаем индикатор (до 85%)
    let ticker_db = db.clone();
    let ticker = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(3)).await;
            let _ = sqlx::query(
                "UPDATE meetings SET progress = LEAST(progress + 3, 85) \
                 WHERE id = $1 AND status = 'processing'",
            )
            .bind(meeting_id)
            .execute(&ticker_db)
            .await;
        }
    });
    let role = std::env::var("DEFAULT_USER_ROLE").unwrap_or_else(|_| "manager".to_string());
    let analysis = analyze_meeting_with_ai(&full_text, &role, &db).await;
    ticker.abort();

    let protocol = match analysis {
        Ok(protocol) => protocol,
        Err(message) => {
            fail_meeting(&db, meeting_id, &message).await;
            return;
        }
    };

    // 3. Сохраняем результат
    set_progress(&db, meeting_id, 90).await;
    let protocol_json = serde_json::to_string(&protocol).unwrap_or_default();
    let _ = sqlx::query("DELETE FROM summaries WHERE meeting_id = $1")
        .bind(meeting_id)
        .execute(&db)
        .await;
    if let Err(err) =
        sqlx::query("INSERT INTO summaries (meeting_id, summary_text) VALUES ($1, $2)")
            .bind(meeting_id)
            .bind(&protocol_json)
            .execute(&db)
            .await
    {
        fail_meeting(&db, meeting_id, &format!("Ошибка БД: {}", err)).await;
        return;
    }

    let mut created_tasks: i64 = 0;
    if create_tasks {
        match crate::api::tasks::create_from_protocol(&db, meeting_id, &title, &protocol.tasks).await {
            Ok(count) => created_tasks = count as i64,
            Err(err) => eprintln!("Не удалось создать задачи из протокола: {}", err),
        }
    }

    let _ = sqlx::query(
        "UPDATE meetings SET status = 'ready', progress = 100, error_message = NULL WHERE id = $1",
    )
    .bind(meeting_id)
    .execute(&db)
    .await;

    // 4. Лента на «Обзоре»
    add_activity(
        &db,
        "task",
        "Готово саммари встречи",
        &format!("«{}»: протокол собран, найдено решений: {}.", title, count_decisions(&protocol)),
        meeting_id,
    )
    .await;
    if created_tasks > 0 {
        add_activity(
            &db,
            "task",
            &format!("Создано задач: {}", created_tasks),
            &format!("По встрече «{}».", title),
            meeting_id,
        )
        .await;
    }
}

fn count_decisions(protocol: &ProtocolResult) -> usize {
    let mut count = 0;
    for item in &protocol.items {
        if item.category == "решено" {
            count += 1;
        }
    }
    count
}

// ============================================================
//  Чтение файлов: txt / docx / pdf
// ============================================================

fn extract_text(file_name: &str, bytes: &[u8]) -> Result<String, String> {
    let lower = file_name.to_lowercase();
    let extension = lower.rsplit('.').next().unwrap_or("");

    match extension {
        "txt" | "md" | "csv" | "vtt" | "srt" | "text" => {
            Ok(String::from_utf8_lossy(bytes).to_string())
        }
        "docx" => extract_docx(bytes),
        "pdf" => pdf_extract::extract_text_from_mem(bytes)
            .map_err(|e| format!("Не удалось прочитать PDF: {}", e)),
        "mp3" | "wav" | "m4a" | "ogg" | "mp4" | "mov" | "webm" | "aac" | "flac" => Err(
            "Распознавание речи из аудио и видео пока не подключено. \
             Загрузите текстовую стенограмму (txt, docx, pdf) или вставьте текст встречи в поле заметок."
                .to_string(),
        ),
        _ => Err(format!(
            "Формат файла «.{}» не поддерживается. Подойдут txt, docx и pdf.",
            extension
        )),
    }
}

fn extract_docx(bytes: &[u8]) -> Result<String, String> {
    let reader = std::io::Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|_| "Файл .docx повреждён".to_string())?;
    let mut xml = String::new();
    {
        let mut entry = archive
            .by_name("word/document.xml")
            .map_err(|_| "В .docx не найден основной текст".to_string())?;
        entry
            .read_to_string(&mut xml)
            .map_err(|_| "Не удалось прочитать текст из .docx".to_string())?;
    }

    // Абзацы и табуляции превращаем в переводы строк/пробелы, остальные теги убираем
    let xml = xml
        .replace("</w:p>", "\n")
        .replace("<w:tab/>", " ")
        .replace("<w:br/>", "\n");
    let mut text = String::new();
    let mut inside_tag = false;
    for symbol in xml.chars() {
        if symbol == '<' {
            inside_tag = true;
        } else if symbol == '>' {
            inside_tag = false;
        } else if !inside_tag {
            text.push(symbol);
        }
    }
    Ok(text
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&"))
}

// ============================================================
//  Вопросы по встрече и экспорт
// ============================================================

#[derive(Deserialize)]
pub struct AskMeetingBody {
    pub question: String,
}

// POST /api/meetings/:id/ask { question } -> { answer, source }
pub async fn ask_about_meeting(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<AskMeetingBody>,
) -> Result<Json<Value>, ResponseError> {
    let row: Option<(String, String, String)> =
        sqlx::query_as("SELECT title, raw_text, status FROM meetings WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await
            .map_err(db_error)?;
    let (title, raw_text, status) =
        row.ok_or_else(|| create_error_response(StatusCode::NOT_FOUND, "Встреча не найдена"))?;
    if status != "ready" {
        return Err(bad_request("Встреча ещё обрабатывается — вопросы станут доступны после готовности"));
    }
    let question = body.question.trim().to_string();
    if question.is_empty() {
        return Err(bad_request("Введите вопрос"));
    }

    let text_part: String = raw_text.chars().take(MAX_TEXT_FOR_MODEL).collect();
    let prompt = format!(
        "Ты — личный секретарь руководителя. Ответь на вопрос строго по тексту встречи «{}». \
         Если в тексте нет ответа, так и скажи, ничего не выдумывай. Отвечай по-русски, кратко.\n\n\
         Текст встречи:\n{}\n\nВопрос: {}",
        title, text_part, question
    );
    let schema = json!({
        "type": "object",
        "properties": { "answer": { "type": "string" } },
        "required": ["answer"]
    });
    let raw = ollama_generate(prompt, schema)
        .await
        .map_err(|e| create_error_response(StatusCode::INTERNAL_SERVER_ERROR, &e))?;
    let parsed: Value = serde_json::from_str(&raw).unwrap_or(json!({ "answer": raw }));
    let answer = parsed["answer"].as_str().unwrap_or(&raw).to_string();

    Ok(Json(json!({ "answer": answer, "source": format!("встреча «{}»", title) })))
}

#[derive(Deserialize)]
pub struct ExportParams {
    pub format: Option<String>,
}

// GET /api/meetings/:id/export?format=txt -> { fileName, content }
pub async fn export_meeting(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<ExportParams>,
) -> Result<Json<Value>, ResponseError> {
    let format = params.format.unwrap_or_else(|| "txt".to_string());
    if format != "txt" && format != "md" {
        return Err(bad_request(
            "Пока доступен экспорт только в текстовом виде (txt). PDF и DOCX ещё не подключены.",
        ));
    }

    let meeting = load_meeting(&state.db, id).await?;
    let mut content = String::new();
    content.push_str(&format!("{}\n", meeting["title"].as_str().unwrap_or("")));
    content.push_str(&format!("Дата: {}\n", meeting["date"].as_str().unwrap_or("")));
    content.push_str(&format!("Отдел: {}\n\n", meeting["department"].as_str().unwrap_or("")));

    let summary = &meeting["summary"];
    if let Some(highlight) = summary["highlight"].as_str() {
        if !highlight.is_empty() {
            content.push_str(&format!("Главное: {}\n\n", highlight));
        }
    }
    if let Some(decisions) = summary["decisions"].as_array() {
        content.push_str("Решения:\n");
        for decision in decisions {
            content.push_str(&format!("- {}\n", decision.as_str().unwrap_or("")));
        }
        content.push('\n');
    }
    if let Some(topics) = summary["topics"].as_array() {
        content.push_str("Темы:\n");
        for topic in topics {
            content.push_str(&format!(
                "- {}: {}\n",
                topic["title"].as_str().unwrap_or(""),
                topic["text"].as_str().unwrap_or("")
            ));
        }
        content.push('\n');
    }

    let task_rows = sqlx::query(
        "SELECT title, description, assignee, due_text FROM tasks WHERE meeting_id = $1 ORDER BY id",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await
    .map_err(db_error)?;
    if !task_rows.is_empty() {
        content.push_str("Задачи:\n");
        for row in task_rows {
            let title: String = row.get("title");
            let description: String = row.get("description");
            let shown = if title.is_empty() { description } else { title };
            content.push_str(&format!(
                "- {} (ответственный: {}; срок: {})\n",
                shown,
                row.get::<Option<String>, _>("assignee").unwrap_or_default(),
                row.get::<String, _>("due_text")
            ));
        }
    }

    Ok(Json(json!({ "fileName": format!("meeting-{}.{}", id, format), "content": content })))
}

// ============================================================
//  Распознавание речи (аудио -> текст)
// ============================================================
//
// Нужен локальный сервер Whisper с OpenAI-совместимым адресом
//   POST {WHISPER_URL}/v1/audio/transcriptions
// (например faster-whisper-server / speaches или LocalAI). Адрес задаётся в .env:
//   WHISPER_URL=http://localhost:8000
//   WHISPER_MODEL=Systran/faster-whisper-small    (необязательно)

struct Segment {
    start_seconds: f64,
    text: String,
}

struct Transcript {
    text: String,
    segments: Vec<Segment>,
    duration_seconds: i32,
}

fn clock_text(total_seconds: f64) -> String {
    let total = total_seconds.max(0.0) as i64;
    format!("{:02}:{:02}", total / 60, total % 60)
}

fn transcript_to_json(segments: &[Segment]) -> Value {
    let mut rows = Vec::new();
    for segment in segments {
        rows.push(json!({
            "time": clock_text(segment.start_seconds),
            "speaker": "Речь",
            "text": segment.text
        }));
    }
    Value::Array(rows)
}

async fn transcribe_audio(file_name: &str, bytes: Vec<u8>) -> Result<Transcript, String> {
    let base = std::env::var("WHISPER_URL").unwrap_or_default();
    if base.trim().is_empty() {
        return Err(
            "Запись сохранена и её можно слушать, но распознавание речи не настроено: \
             запустите сервер Whisper и укажите WHISPER_URL в .env. \
             Пока можно вставить текст встречи в заметки."
                .to_string(),
        );
    }
    let model = std::env::var("WHISPER_MODEL").unwrap_or_else(|_| "whisper-1".to_string());
    let url = format!("{}/v1/audio/transcriptions", base.trim_end_matches('/'));

    let file_part = reqwest::multipart::Part::bytes(bytes).file_name(file_name.to_string());
    let form = reqwest::multipart::Form::new()
        .part("file", file_part)
        .text("model", model)
        .text("language", "ru")
        .text("response_format", "verbose_json");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3600))
        .build()
        .map_err(|e| format!("Ошибка HTTP-клиента: {}", e))?;
    let response = client
        .post(url)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Не удалось связаться с сервером распознавания речи (WHISPER_URL): {}", e))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Сервер распознавания речи вернул {}: {}", status, body));
    }
    let data: Value = response
        .json()
        .await
        .map_err(|e| format!("Непонятный ответ сервера распознавания речи: {}", e))?;

    let mut segments: Vec<Segment> = Vec::new();
    if let Some(list) = data["segments"].as_array() {
        for item in list {
            let text = item["text"].as_str().unwrap_or("").trim().to_string();
            if !text.is_empty() {
                segments.push(Segment {
                    start_seconds: item["start"].as_f64().unwrap_or(0.0),
                    text,
                });
            }
        }
    }
    let full_text = data["text"].as_str().unwrap_or("").trim().to_string();
    if segments.is_empty() && !full_text.is_empty() {
        segments.push(Segment { start_seconds: 0.0, text: full_text.clone() });
    }
    if segments.is_empty() {
        return Err("В записи не удалось распознать речь".to_string());
    }

    let mut text = String::new();
    for segment in &segments {
        text.push_str(&segment.text);
        text.push('\n');
    }
    let duration = data["duration"].as_f64().unwrap_or(0.0);
    Ok(Transcript { text, segments, duration_seconds: duration.round() as i32 })
}
