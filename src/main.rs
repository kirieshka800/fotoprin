use axum::{
    extract::{DefaultBodyLimit, State},
    http::{header, HeaderMap, StatusCode},
    response::Redirect,
    middleware,
    routing::{get, post},
    Json, Router,
};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::env;
use tower_http::services::ServeDir;

mod ai;
mod assistant;
mod api;
mod auth;
mod error;
mod meetings;
mod util;
mod transcription;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Meeting {
    pub id: i64,
    pub title: String,
    pub raw_text: String,
    pub created_at: NaiveDateTime,
    #[sqlx(skip)]
    pub protocol: Option<ProtocolResult>,
}

#[derive(Debug, Deserialize)]
pub struct CreateMeetingPayload {
    pub title: String,
    pub raw_text: String,
    pub user_role: String,
}

#[derive(Debug, Deserialize)]
pub struct ConfirmMeetingPayload {
    pub title: String,
    pub raw_text: String,
    pub protocol: ProtocolResult,
}

#[derive(Debug, Deserialize)]
pub struct AskPayload {
    pub query: String,
    pub user_role: String,
}

#[derive(Debug, Serialize)]
pub struct DocumentSource {
    pub title: String,
    pub snippet: String,
    pub date: String,
}

#[derive(Debug, Serialize)]
pub struct AskResponse {
    pub answer: String,
    pub sources: Vec<DocumentSource>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProtocolItem {
    pub category: String,
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TaskItem {
    pub formulation: String,
    pub owner: String,
    pub deadline: String,
    pub basis: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProtocolResult {
    /// Главная мысль встречи (1–2 предложения). У старых протоколов поля нет — тогда пусто.
    #[serde(default)]
    pub highlight: String,
    pub items: Vec<ProtocolItem>,
    pub tasks: Vec<TaskItem>,
}

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool,
    format: serde_json::Value,
    options: serde_json::Value,
}

#[derive(Deserialize, Debug)]
struct OllamaResponse {
    response: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

pub type ResponseError = (StatusCode, HeaderMap, Json<ErrorResponse>);

pub fn create_error_response(status: StatusCode, message: &str) -> ResponseError {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "application/json".parse().unwrap());
    (
        status,
        headers,
        Json(ErrorResponse {
            error: message.to_string(),
        }),
    )
}

/// Поиск корпоративных документов для анализа встреч с учетом ролей (AI-02)
async fn search_company_docs(query: &str, user_role: &str, db: &PgPool) -> Vec<String> {
    let rows: Result<Vec<(String,)>, _> = sqlx::query_as(
        r#"
        SELECT content FROM company_documents
        WHERE (title ILIKE $1 OR content ILIKE $1)
          AND $2 = ANY(allowed_roles)
        LIMIT 3
        "#,
    )
        .bind(format!("%{}%", query))
        .bind(user_role)
        .fetch_all(db)
        .await;

    match rows {
        Ok(res) => res.into_iter().map(|r| r.0).collect(),
        Err(_) => vec![],
    }
}

/// Умный поиск документов и последних протоколов встреч (действует как секретарь)
async fn search_docs_with_metadata(_query: &str, user_role: &str, db: &PgPool) -> Vec<DocumentSource> {
    let mut sources = vec![];

    // 1. Ищем в официальных корпоративных документах с учетом ролевой модели
    let doc_rows: Result<Vec<(String, String, NaiveDateTime)>, _> = sqlx::query_as(
        r#"
        SELECT title, content, created_at FROM company_documents
        WHERE $1 = ANY(allowed_roles)
        LIMIT 5
        "#,
    )
        .bind(user_role)
        .fetch_all(db)
        .await;

    if let Ok(rows) = doc_rows {
        for r in rows {
            sources.push(DocumentSource {
                title: format!("Документ: {}", r.0),
                snippet: r.1,
                date: r.2.format("%Y-%m-%d %H:%M").to_string(),
            });
        }
    }

    // 2. Подтягиваем последние протоколы встреч целиком, чтобы секретарь знал всю текучку
    let meeting_rows: Result<Vec<(String, String, NaiveDateTime)>, _> = sqlx::query_as(
        r#"
        SELECT m.title, s.summary_text, m.created_at
        FROM meetings m
        JOIN summaries s ON m.id = s.meeting_id
        ORDER BY m.created_at DESC
        LIMIT 5
        "#,
    )
        .fetch_all(db)
        .await;

    if let Ok(rows) = meeting_rows {
        for r in rows {
            sources.push(DocumentSource {
                title: format!("Протокол встречи: {}", r.0),
                snippet: r.1,
                date: r.2.format("%Y-%m-%d %H:%M").to_string(),
            });
        }
    }

    sources
}

fn mask_sensitive_data(input: &str) -> String {
    input
        .replace("зарплат", "[ЗАСЕКРЕЧЕНО]")
        .replace("паспорт", "[ЗАСЕКРЕЧЕНО]")
}

async fn log_audit_event(
    db: &PgPool,
    action: &str,
    user_role: &str,
    details: &str,
    status: &str,
) {
    let safe_details = mask_sensitive_data(details);
    let _ = sqlx::query(
        r#"
        INSERT INTO audit_logs (action, user_role, details, status, created_at)
        VALUES ($1, $2, $3, $4, NOW())
        "#,
    )
        .bind(action)
        .bind(user_role)
        .bind(safe_details)
        .bind(status)
        .execute(db)
        .await;
}


/// Настройки Ollama берутся из .env (есть значения по умолчанию)
pub fn ollama_model_name() -> String {
    env::var("OLLAMA_MODEL").unwrap_or_else(|_| "qwen2.5:7b".to_string())
}

fn ollama_generate_url() -> String {
    let base = env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());
    ai::generate_url(&base)
}

/// Один запрос к Ollama: возвращает строку с JSON, подходящим под schema
pub async fn ollama_generate(prompt: String, schema: serde_json::Value) -> Result<String, String> {
    let num_ctx: u32 = env::var("OLLAMA_NUM_CTX")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(8192);

    let payload = OllamaRequest {
        model: ollama_model_name(),
        prompt,
        stream: false,
        format: schema,
        options: serde_json::json!({ "num_ctx": num_ctx }),
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(900))
        .build()
        .map_err(|e| format!("Ошибка HTTP-клиента: {}", e))?;

    let res = client
        .post(ollama_generate_url())
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Не удалось связаться с Ollama (запущена ли она?): {}", e))?;

    let status = res.status();
    if !status.is_success() {
        let err_body = res.text().await.unwrap_or_default();
        return Err(format!("Ollama вернула статус {}: {}", status, err_body));
    }

    let ollama_res = res
        .json::<OllamaResponse>()
        .await
        .map_err(|e| format!("Ошибка парсинга ответа от Ollama: {}", e))?;

    if let Some(error) = ollama_res.error {
        return Err(format!("Ollama: {}", error));
    }
    ollama_res
        .response
        .ok_or_else(|| "Ollama вернула пустой ответ".to_string())
}

pub async fn analyze_meeting_with_ai(
    raw_text: &str,
    user_role: &str,
    db: &PgPool,
) -> Result<ProtocolResult, String> {
    // Длинные стенограммы обрезаем, чтобы не переполнить контекст модели
    let raw_text: String = raw_text.chars().take(14000).collect();
    let docs = search_company_docs(&raw_text.chars().take(200).collect::<String>(), user_role, db).await;
    let context_section = if docs.is_empty() {
        "".to_string()
    } else {
        format!("\nНайденные корпоративные документы:\n{}\n", docs.join("\n---\n"))
    };

    let prompt = format!(
        r#"Ты — профессиональный корпоративный ИИ-ассистент компании «Фотопринт». Проанализируй текст стенограммы встречи и строго распредели информацию.{}

КРИТЕРИИ РАЗДЕЛЕНИЯ (AI-04, AI-05):
1. Массив "items" (Протокол встречи):
   - Сюда пиши ВСЕ факты, договоренности, идеи и темы.
   - Категория (category) должна быть строго одной из четырех:
     * "решено" — утвержденные решения и факты.
     * "предложено" — идеи, озвученные участниками, но пока без реализации.
     * "обсуждалось" — текущие проблемы или темы на повестке.
     * "требует уточнения" — нерешенные вопросы.

2. Массив "tasks" (Реестр задач):
   - Сюда включай ИСКЛЮЧЕНИЯ: только конкретные поручения, по которым требуется выполнить физическое или организационное действие.
   - Идеи «просто обдумать» или информационные сообщения задачами НЕ являются и в "tasks" НЕ попадают!
   - Если в тексте явно не указан ответственный или дедлайн для задачи, ты ОБЯЗАН вписать туда ровно фразу: "требует уточнения". Ничего не выдумывай (AI-05).

3. Язык: Строго русский язык.

4. Поле "highlight" — главная мысль встречи в 1–2 предложениях.

Верни результат СТРОГО в формате JSON:
{{
  "highlight": "...",
  "items": [{{"category": "...", "text": "..."}}],
  "tasks": [{{"formulation": "...", "owner": "...", "deadline": "...", "basis": "..."}}]
}}

Текст для анализа:
{}
"#,
        context_section, raw_text
    );

    let schema = serde_json::json!({
      "type": "object",
      "properties": {
        "highlight": { "type": "string" },
        "items": {
          "type": "array",
          "items": {
            "type": "object",
            "properties": {
              "category": {
                "type": "string",
                "enum": ["решено", "предложено", "обсуждалось", "требует уточнения"]
              },
              "text": { "type": "string" }
            },
            "required": ["category", "text"]
          }
        },
        "tasks": {
          "type": "array",
          "items": {
            "type": "object",
            "properties": {
              "formulation": { "type": "string" },
              "owner": { "type": "string" },
              "deadline": { "type": "string" },
              "basis": { "type": "string" }
            },
            "required": ["formulation", "owner", "deadline", "basis"]
          }
        }
      },
      "required": ["highlight", "items", "tasks"]
    });

    let raw_json_str = ollama_generate(prompt, schema).await?;

    let protocol_result: ProtocolResult = serde_json::from_str(&raw_json_str)
        .map_err(|e| format!("Ошибка десериализации JSON от Ollama: {}", e))?;

    Ok(protocol_result)
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let database_url = env::var("DATABASE_URL")
        .expect("ОШИБКА: Задайте переменную окружения DATABASE_URL в .env");

    let db = PgPool::connect(&database_url)
        .await
        .expect("ОШИБКА: Не удалось подключиться к базе данных PostgreSQL");

    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("ОШИБКА: Не удалось применить миграции к базе данных");

    println!("✅ Миграции успешно применены!");

    let state = AppState { db };

    meetings::mark_stuck_meetings(&state.db).await;
    tokio::spawn(api::reminders::run_worker(state.db.clone()));

    // Показываем, откуда раздаются файлы сайта, чтобы было видно, если что-то не найдено
    let static_dir = find_static_dir();
    println!("📁 Файлы сайта раздаются из: {}", static_dir.display());
    for file_name in ["overview.html", "login.html", "register.html", "ui.js", "api.js", "config.js", "mock-data.js", "styles.css"] {
        let exists = static_dir.join(file_name).is_file();
        println!("   {} {}", if exists { "✅" } else { "❌ НЕТ ФАЙЛА:" }, file_name);
    }

    // Всё, что внутри этого роутера, доступно только после входа
    let protected = Router::new()
        // Обзор
        .route("/api/me", get(auth::get_me))
        .route("/api/settings", get(meetings::get_settings).put(meetings::put_settings))
        .route("/api/stats", get(meetings::get_stats))
        .route("/api/activity", get(meetings::get_activity))
        // Справочники
        .route("/api/employees", get(meetings::list_employees).post(meetings::create_employee))
        .route("/api/departments", get(meetings::list_departments))
        // Встречи
        .route("/api/meetings", get(meetings::list_meetings).post(meetings::create_meeting))
        .route("/api/meetings/preview", post(preview_meeting))
        .route("/api/meetings/confirm", post(confirm_meeting))
        .route(
            "/api/meetings/:id",
            get(meetings::get_meeting).delete(meetings::delete_meeting),
        )
        .route("/api/meetings/:id/status", get(meetings::meeting_status))
        .route("/api/meetings/:id/transcribe", post(meetings::retry_transcription))
        .route("/api/meetings/:id/ask", post(meetings::ask_about_meeting))
        .route("/api/meetings/:id/export", get(meetings::export_meeting))
        // Остальное (было раньше)
        .route("/api/ask", post(ask_knowledge_base))
        // Задачи, график работы, уведомления
        .merge(api::router())
        .merge(assistant::router())
        // Файлы фронтенда отдаём ТОЛЬКО из папки static (так .env недоступен из браузера)
        .nest_service("/uploads", ServeDir::new(meetings::UPLOADS_DIR))
        .layer(middleware::from_fn_with_state(state.clone(), auth::require_auth));

    // Открытые адреса: главная, проверка, вход/регистрация и файлы страниц
    let app = Router::new()
        .route("/", get(home_redirect))
        .route("/health", get(health_check))
        .route("/api/auth/register", post(auth::register))
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/logout", post(auth::logout))
        .merge(protected)
        .fallback_service(ServeDir::new(&static_dir))
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(middleware::from_fn(disable_browser_cache))
        .with_state(state);

    let address = "127.0.0.1:8081";
    let listener = tokio::net::TcpListener::bind(address).await.unwrap();

    println!("🚀 Сервер запущен на http://{}", address);
    axum::serve(listener, app).await.unwrap();
}

// Always serve the current authenticated application at the ordinary URLs.
async fn disable_browser_cache(request: axum::extract::Request, next: middleware::Next) -> axum::response::Response {
    let mut response=next.run(request).await;
    response.headers_mut().insert(header::CACHE_CONTROL, axum::http::HeaderValue::from_static("no-store"));
    response
}

async fn home_redirect() -> Redirect {
    Redirect::to("/overview.html")
}

async fn health_check() -> &'static str {
    "Fotoprint Backend is running! (сборка: входы + аудио)"
}

/// Ищет папку static: сначала рядом с рабочей папкой запуска, потом рядом с Cargo.toml
fn find_static_dir() -> std::path::PathBuf {
    if let Ok(custom) = env::var("STATIC_DIR") {
        return std::path::PathBuf::from(custom);
    }
    let in_current = std::path::PathBuf::from("static");
    if in_current.is_dir() {
        return in_current;
    }
    let next_to_manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("static");
    if next_to_manifest.is_dir() {
        return next_to_manifest;
    }
    in_current
}

async fn preview_meeting(
    State(state): State<AppState>,
    Json(payload): Json<CreateMeetingPayload>,
) -> Result<Json<ProtocolResult>, ResponseError> {
    let protocol = analyze_meeting_with_ai(&payload.raw_text, &payload.user_role, &state.db)
        .await
        .map_err(|err| create_error_response(StatusCode::INTERNAL_SERVER_ERROR, &err))?;

    log_audit_event(
        &state.db,
        "PREVIEW_MEETING",
        &payload.user_role,
        &payload.raw_text,
        "SUCCESS",
    )
        .await;

    Ok(Json(protocol))
}

async fn confirm_meeting(
    State(state): State<AppState>,
    Json(payload): Json<ConfirmMeetingPayload>,
) -> Result<(StatusCode, Json<Meeting>), ResponseError> {
    let meeting = sqlx::query_as::<_, Meeting>(
        r#"
        INSERT INTO meetings (title, raw_text)
        VALUES ($1, $2)
        RETURNING id, title, raw_text, created_at
        "#,
    )
        .bind(&payload.title)
        .bind(&payload.raw_text)
        .fetch_one(&state.db)
        .await
        .map_err(|err| create_error_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("Ошибка БД: {}", err)))?;

    let proto_json = serde_json::to_string(&payload.protocol).unwrap_or_default();

    let _ = sqlx::query(
        r#"
        INSERT INTO summaries (meeting_id, summary_text)
        VALUES ($1, $2)
        "#,
    )
        .bind(meeting.id)
        .bind(proto_json)
        .execute(&state.db)
        .await;

    // Задачи из реестра попадают в раздел «Задачи» (ответственный, срок, напоминание, уведомления)
    if let Err(err) = api::tasks::create_from_protocol(
        &state.db,
        meeting.id,
        &meeting.title,
        &payload.protocol.tasks,
    )
    .await
    {
        eprintln!("Не удалось создать задачи из протокола: {}", err);
    }

    log_audit_event(&state.db, "CONFIRM_MEETING", "system", &payload.title, "SAVED").await;

    let response_meeting = Meeting {
        id: meeting.id,
        title: meeting.title,
        raw_text: meeting.raw_text,
        created_at: meeting.created_at,
        protocol: Some(payload.protocol),
    };

    Ok((StatusCode::CREATED, Json(response_meeting)))
}

/// Эндпоинт для ответов на вопросы по базе знаний (AI-01, AI-02, AI-05) с ролью личного секретаря
async fn ask_knowledge_base(
    State(state): State<AppState>,
    Json(payload): Json<AskPayload>,
) -> Result<Json<AskResponse>, ResponseError> {
    let sources = search_docs_with_metadata(&payload.query, &payload.user_role, &state.db).await;

    let context_text = if sources.is_empty() {
        "В системе пока нет сохраненных документов и встреч.".to_string()
    } else {
        sources
            .iter()
            .map(|s| format!("Источник: '{}' от {}\nСодержание: {}", s.title, s.date, s.snippet))
            .collect::<Vec<_>>()
            .join("\n---\n")
    };

    let prompt = format!(
        r#"Ты — профессиональный личный секретарь руководителей типографии «Фотопринт». Ответь на вопрос пользователя человеческим, вежливым и экспертным языком, опираясь строго на предоставленные ниже корпоративные документы и протоколы встреч.

ПРАВИЛА (AI-01, AI-05):
1. Отвечай по существу, обобщая задачи и решения из переданных протоколов и документов.
2. Не выдумывай факты от себя.
3. В конце ответа обязательно укажи использованные источники.

База знаний и протоколы:
{0}

Вопрос пользователя:
{1}
"#,
        context_text, payload.query
    );

    let raw_answer = ollama_generate(
        prompt,
        serde_json::json!({
            "type": "object",
            "properties": { "answer": { "type": "string" } },
            "required": ["answer"]
        }),
    )
    .await
    .map_err(|e| create_error_response(StatusCode::INTERNAL_SERVER_ERROR, &e))?;

    let answer_json: serde_json::Value = serde_json::from_str(&raw_answer).unwrap_or(serde_json::json!({
        "answer": raw_answer
    }));
    let final_answer = answer_json["answer"].as_str().unwrap_or(&raw_answer).to_string();

    log_audit_event(&state.db, "ASK_KNOWLEDGE_BASE", &payload.user_role, &payload.query, "SUCCESS").await;

    Ok(Json(AskResponse {
        answer: final_answer,
        sources,
    }))
}