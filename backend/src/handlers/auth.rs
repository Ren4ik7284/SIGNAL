use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::auth::{
    create_jwt, extract_claims_from_headers, hash_password, validate_email, validate_password,
    validate_username, verify_password, AuthResponse, LoginRequest, SendCodeRequest, UserInfo,
    VerifyCodeRequest,
};
use crate::email::send_verification_email;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct ResendCodeRequest {
    pub email: String,
}

pub async fn send_verification_code(
    State(state): State<AppState>,
    Json(payload): Json<SendCodeRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if let Err(err) = validate_username(&payload.username) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": err }))));
    }

    if let Err(err) = validate_password(&payload.password) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": err }))));
    }

    let clean_email = match validate_email(&payload.email) {
        Ok(e) => e,
        Err(err) => return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": err })))),
    };

    let clean_username = payload.username.trim();

    let user_by_name = sqlx::query("SELECT id FROM users WHERE username = ?")
        .bind(clean_username)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Ошибка базы данных" })),
            )
        })?;

    if user_by_name.is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({ "error": "Пользователь с таким именем уже существует" })),
        ));
    }

    let user_by_email = sqlx::query("SELECT id FROM users WHERE email = ?")
        .bind(&clean_email)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Ошибка базы данных" })),
            )
        })?;

    if user_by_email.is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({ "error": "Аккаунт с таким адресом email уже зарегистрирован" })),
        ));
    }

    let password_hash = hash_password(payload.password.trim()).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Не удалось захэшировать пароль" })),
        )
    })?;

    let code = format!("{:06}", fastrand::u32(100000..=999999));
    let now = chrono::Utc::now().timestamp();
    let expires_at = now + 900;

    sqlx::query(
        r#"
        INSERT INTO email_verifications (email, code, username, password_hash, expires_at, created_at, attempts)
        VALUES (?, ?, ?, ?, ?, ?, 0)
        ON CONFLICT(email) DO UPDATE SET
            code = excluded.code,
            username = excluded.username,
            password_hash = excluded.password_hash,
            expires_at = excluded.expires_at,
            created_at = excluded.created_at,
            attempts = 0
        "#,
    )
    .bind(&clean_email)
    .bind(&code)
    .bind(clean_username)
    .bind(&password_hash)
    .bind(expires_at)
    .bind(now)
    .execute(&state.pool)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Не удалось сохранить код верификации" })),
        )
    })?;

    let _ = send_verification_email(&clean_email, &code).await;

    Ok(Json(json!({
        "success": true,
        "email": clean_email,
        "message": "Код подтверждения отправлен на вашу почту",
        "expires_in": 900
    })))
}

pub async fn resend_verification_code(
    State(state): State<AppState>,
    Json(payload): Json<ResendCodeRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let clean_email = match validate_email(&payload.email) {
        Ok(e) => e,
        Err(err) => return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": err })))),
    };

    let record = sqlx::query("SELECT username, password_hash, created_at FROM email_verifications WHERE email = ?")
        .bind(&clean_email)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Ошибка базы данных" })),
            )
        })?;

    let row = match record {
        Some(r) => r,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Сессия верификации не найдена. Начните регистрацию заново." })),
            ))
        }
    };

    let now = chrono::Utc::now().timestamp();
    let created_at: i64 = row.get("created_at");
    if (now - created_at) < 30 {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "Пожалуйста, подождите перед повторной отправкой кода" })),
        ));
    }

    let code = format!("{:06}", fastrand::u32(100000..=999999));
    let expires_at = now + 900;

    sqlx::query("UPDATE email_verifications SET code = ?, expires_at = ?, created_at = ?, attempts = 0 WHERE email = ?")
        .bind(&code)
        .bind(expires_at)
        .bind(now)
        .bind(&clean_email)
        .execute(&state.pool)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Не удалось обновить код" })),
            )
        })?;

    let _ = send_verification_email(&clean_email, &code).await;

    Ok(Json(json!({
        "success": true,
        "message": "Новый код подтверждения отправлен на email"
    })))
}

pub async fn verify_registration_code(
    State(state): State<AppState>,
    Json(payload): Json<VerifyCodeRequest>,
) -> Result<Json<AuthResponse>, (StatusCode, Json<Value>)> {
    let clean_email = match validate_email(&payload.email) {
        Ok(e) => e,
        Err(err) => return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": err })))),
    };

    let submitted_code = payload.code.trim();
    if submitted_code.len() != 6 || !submitted_code.chars().all(|c| c.is_ascii_digit()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Код подтверждения должен состоять из 6 цифр" })),
        ));
    }

    let record = sqlx::query(
        "SELECT code, username, password_hash, expires_at, attempts FROM email_verifications WHERE email = ?",
    )
    .bind(&clean_email)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Ошибка базы данных" })),
        )
    })?;

    let row = match record {
        Some(r) => r,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Код подтверждения не найден или истек. Запросите регистрацию заново." })),
            ))
        }
    };

    let db_code: String = row.get("code");
    let username: String = row.get("username");
    let password_hash: String = row.get("password_hash");
    let expires_at: i64 = row.get("expires_at");
    let attempts: i64 = row.get("attempts");

    let now = chrono::Utc::now().timestamp();
    if now > expires_at {
        let _ = sqlx::query("DELETE FROM email_verifications WHERE email = ?")
            .bind(&clean_email)
            .execute(&state.pool)
            .await;

        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Срок действия кода истек. Запросите новый код." })),
        ));
    }

    if attempts >= 5 {
        let _ = sqlx::query("DELETE FROM email_verifications WHERE email = ?")
            .bind(&clean_email)
            .execute(&state.pool)
            .await;

        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "Превышено количество попыток ввода. Запросите новый код." })),
        ));
    }

    if db_code != submitted_code {
        let _ = sqlx::query("UPDATE email_verifications SET attempts = attempts + 1 WHERE email = ?")
            .bind(&clean_email)
            .execute(&state.pool)
            .await;

        let left = 4 - attempts;
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("Неверный код подтверждения. Осталось попыток: {}", if left > 0 { left } else { 0 })
            })),
        ));
    }

    let user_id = Uuid::new_v4().to_string();
    let created_at = now;

    sqlx::query(
        "INSERT INTO users (id, username, email, password_hash, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&user_id)
    .bind(&username)
    .bind(&clean_email)
    .bind(&password_hash)
    .bind(created_at)
    .execute(&state.pool)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Не удалось зарегистрировать пользователя" })),
        )
    })?;

    let _ = sqlx::query("DELETE FROM email_verifications WHERE email = ?")
        .bind(&clean_email)
        .execute(&state.pool)
        .await;

    let token = create_jwt(&user_id, &username, Some(&clean_email)).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Не удалось сгенерировать токен авторизации" })),
        )
    })?;

    Ok(Json(AuthResponse {
        token,
        user: UserInfo {
            id: user_id,
            username,
            email: Some(clean_email),
        },
    }))
}

pub async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, (StatusCode, Json<Value>)> {
    let login = payload.login.trim();
    let password = payload.password.trim();

    if login.is_empty() || login.contains(' ') || login.contains('\t') || login.contains('\n') {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Логин или email не должен содержать пробелов" })),
        ));
    }

    if password.is_empty() || password.contains(' ') || password.contains('\t') || password.contains('\n') {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Пароль не должен содержать пробелов" })),
        ));
    }

    let row = sqlx::query("SELECT id, username, email, password_hash FROM users WHERE username = ? OR email = ?")
        .bind(login)
        .bind(login.to_lowercase())
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Ошибка базы данных" })),
            )
        })?;

    let user_row = match row {
        Some(r) => r,
        None => {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "Неверный логин/email или пароль" })),
            ))
        }
    };

    let user_id: String = user_row.get("id");
    let db_username: String = user_row.get("username");
    let db_email: Option<String> = user_row.get("email");
    let password_hash: String = user_row.get("password_hash");

    if !verify_password(password, &password_hash) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Неверный логин/email или пароль" })),
        ));
    }

    let token = create_jwt(&user_id, &db_username, db_email.as_deref()).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Не удалось сгенерировать токен авторизации" })),
        )
    })?;

    Ok(Json(AuthResponse {
        token,
        user: UserInfo {
            id: user_id,
            username: db_username,
            email: db_email,
        },
    }))
}

pub async fn get_me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<UserInfo>, (StatusCode, Json<Value>)> {
    let claims = extract_claims_from_headers(&headers).map_err(|(code, msg)| {
        (code, Json(json!({ "error": msg })))
    })?;

    let row = sqlx::query("SELECT id, username, email FROM users WHERE id = ?")
        .bind(&claims.sub)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Ошибка базы данных" })),
            )
        })?;

    match row {
        Some(r) => {
            let id: String = r.get("id");
            let username: String = r.get("username");
            let email: Option<String> = r.get("email");
            Ok(Json(UserInfo { id, username, email }))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Пользователь не найден" })),
        )),
    }
}
