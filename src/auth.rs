// Регистрация, вход, выход и проверка сессии.
//   POST /api/auth/register { name, email, password }  -> { token, user }
//   POST /api/auth/login    { email, password }        -> { token, user }
//   POST /api/auth/logout
//   GET  /api/me                                       -> текущий пользователь
// Сессия хранится в cookie "fp_session" (HttpOnly), токен можно также передать
// заголовком "Authorization: Bearer <token>".

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Extension, Json,
};
use rand::RngCore;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{create_error_response, AppState, ResponseError};

const COOKIE_NAME: &str = "fp_session";
const SESSION_DAYS: i64 = 30;

#[derive(Clone, Debug)]
pub struct CurrentUser {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub role: String,
    pub position: String,
}

fn initials_of(name: &str) -> String {
    let mut result = String::new();
    for word in name.split_whitespace().take(2) {
        if let Some(letter) = word.chars().next() {
            result.extend(letter.to_uppercase());
        }
    }
    if result.is_empty() {
        result.push('П');
    }
    result
}

fn user_json(user: &CurrentUser) -> Value {
    json!({
        "id": user.id,
        "name": user.name,
        "initials": initials_of(&user.name),
        "role": user.position,
        "appRole": user.role,
        "email": user.email
    })
}

fn db_error(err: sqlx::Error) -> ResponseError {
    create_error_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("Ошибка БД: {}", err))
}

fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| format!("Не удалось защитить пароль: {}", e))
}

fn password_matches(password: &str, stored_hash: &str) -> bool {
    match PasswordHash::new(stored_hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

fn session_cookie(token: &str, max_age_seconds: i64) -> String {
    format!(
        "{}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}",
        COOKIE_NAME, token, max_age_seconds
    )
}

fn with_cookie(cookie: String, body: Value) -> Response {
    let mut response = Json(body).into_response();
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

// Создаёт сессию в БД и возвращает случайный токен
async fn create_session(state: &AppState, user_id: i64) -> Result<String, ResponseError> {
    let mut random_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut random_bytes);
    let token = hex::encode(random_bytes);

    // заодно убираем просроченные сессии
    let _ = sqlx::query("DELETE FROM sessions WHERE expires_at < NOW()")
        .execute(&state.db)
        .await;

    sqlx::query(
        "INSERT INTO sessions (token_hash, user_id, expires_at) \
         VALUES ($1, $2, NOW() + make_interval(days => $3))",
    )
    .bind(hash_token(&token))
    .bind(user_id)
    .bind(SESSION_DAYS as i32)
    .execute(&state.db)
    .await
    .map_err(db_error)?;
    Ok(token)
}

#[derive(Deserialize)]
pub struct RegisterBody {
    pub name: String,
    pub email: String,
    pub password: String,
}

// POST /api/auth/register
pub async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterBody>,
) -> Result<Response, ResponseError> {
    let name = body.name.trim().to_string();
    let email = body.email.trim().to_lowercase();

    if name.is_empty() {
        return Err(create_error_response(StatusCode::BAD_REQUEST, "Введите имя"));
    }
    if !email.contains('@') || !email.contains('.') || email.len() < 5 {
        return Err(create_error_response(StatusCode::BAD_REQUEST, "Введите корректную почту"));
    }
    if body.password.chars().count() < 8 {
        return Err(create_error_response(
            StatusCode::BAD_REQUEST,
            "Пароль должен быть не короче 8 символов",
        ));
    }

    // Первый зарегистрированный пользователь становится администратором
    let users_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await
        .map_err(db_error)?;
    let (role, position) = if users_count == 0 {
        ("admin", "Руководитель команды")
    } else {
        ("manager", "Сотрудник")
    };

    let password_hash = hash_password(&body.password)
        .map_err(|e| create_error_response(StatusCode::INTERNAL_SERVER_ERROR, &e))?;

    let inserted: Result<i64, sqlx::Error> = sqlx::query_scalar(
        "INSERT INTO users (email, name, password_hash, role, position) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(&email)
    .bind(&name)
    .bind(&password_hash)
    .bind(role)
    .bind(position)
    .fetch_one(&state.db)
    .await;

    let user_id = match inserted {
        Ok(id) => id,
        Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
            return Err(create_error_response(
                StatusCode::CONFLICT,
                "Эта почта уже зарегистрирована. Попробуйте войти.",
            ));
        }
        Err(err) => return Err(db_error(err)),
    };

    let token = create_session(&state, user_id).await?;
    let user = CurrentUser {
        id: user_id,
        name,
        email,
        role: role.to_string(),
        position: position.to_string(),
    };
    Ok(with_cookie(
        session_cookie(&token, SESSION_DAYS * 24 * 3600),
        json!({ "token": token, "user": user_json(&user) }),
    ))
}

#[derive(Deserialize)]
pub struct LoginBody {
    pub email: String,
    pub password: String,
}

// POST /api/auth/login
pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginBody>,
) -> Result<Response, ResponseError> {
    let email = body.email.trim().to_lowercase();

    let row: Option<(i64, String, String, String, String, String)> = sqlx::query_as(
        "SELECT id, name, email, role, position, password_hash FROM users WHERE email = $1",
    )
    .bind(&email)
    .fetch_optional(&state.db)
    .await
    .map_err(db_error)?;

    // Ошибку специально делаем одинаковой для «нет такой почты» и «неверный пароль»
    let wrong = || {
        // Код 400, а не 401: на 401 страница отправила бы обратно на вход
        create_error_response(StatusCode::BAD_REQUEST, "Неверная почта или пароль")
    };
    let (id, name, email, role, position, password_hash) = row.ok_or_else(wrong)?;
    if !password_matches(&body.password, &password_hash) {
        return Err(wrong());
    }

    let token = create_session(&state, id).await?;
    let user = CurrentUser { id, name, email, role, position };
    Ok(with_cookie(
        session_cookie(&token, SESSION_DAYS * 24 * 3600),
        json!({ "token": token, "user": user_json(&user) }),
    ))
}

fn tokens_from_headers(headers: &HeaderMap) -> Vec<String> {
    let mut tokens = Vec::new();
    if let Some(value) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(token) = value.strip_prefix("Bearer ") {
            tokens.push(token.trim().to_string());
        }
    }
    if let Some(value) = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()) {
        for part in value.split(';') {
            let part = part.trim();
            if let Some(token) = part.strip_prefix("fp_session=") {
                tokens.push(token.to_string());
            }
        }
    }
    tokens
}

async fn find_user(state: &AppState, headers: &HeaderMap) -> Option<CurrentUser> {
    for token in tokens_from_headers(headers) {
        let row: Option<(i64, String, String, String, String)> = sqlx::query_as(
            "SELECT u.id, u.name, u.email, u.role, u.position FROM sessions s \
             JOIN users u ON u.id = s.user_id \
             WHERE s.token_hash = $1 AND s.expires_at > NOW()",
        )
        .bind(hash_token(&token))
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
        if let Some((id, name, email, role, position)) = row {
            return Some(CurrentUser { id, name, email, role, position });
        }
    }
    None
}

// Проверка входа для всех защищённых адресов
pub async fn require_auth(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    match find_user(&state, request.headers()).await {
        Some(user) => {
            request.extensions_mut().insert(user);
            next.run(request).await
        }
        None => create_error_response(StatusCode::UNAUTHORIZED, "Нужно войти в аккаунт").into_response(),
    }
}

// POST /api/auth/logout
pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> Response {
    for token in tokens_from_headers(&headers) {
        let _ = sqlx::query("DELETE FROM sessions WHERE token_hash = $1")
            .bind(hash_token(&token))
            .execute(&state.db)
            .await;
    }
    with_cookie(session_cookie("", 0), json!({ "ok": true }))
}

// GET /api/me
pub async fn get_me(Extension(user): Extension<CurrentUser>) -> Json<Value> {
    Json(user_json(&user))
}
