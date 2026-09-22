use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::time::Instant;
use uuid::Uuid;

use crate::auth::{
    create_jwt, extract_claims_from_headers, hash_password, validate_email, validate_password,
    validate_username, verify_password, AuthResponse, ConfirmResetPasswordRequest, LoginRequest,
    RegisterRequest, ResetPasswordRequest, SendCodeRequest, UserInfo, VerifyCodeRequest,
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

    send_verification_email(&clean_email, &code).await.map_err(|err_msg| {
        eprintln!("[SIGNAL AUTH] Ошибка отправки письма на {}: {}", clean_email, err_msg);
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({ 
                "error": "Не удалось отправить письмо с кодом. Убедитесь в правильности email или настройте отправку писем на сервере." 
            })),
        )
    })?;

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

    send_verification_email(&clean_email, &code).await.map_err(|err_msg| {
        eprintln!("[SIGNAL AUTH] Ошибка повторной отправки письма на {}: {}", clean_email, err_msg);
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "Не удалось отправить письмо с кодом. Попробуйте позже." })),
        )
    })?;

    Ok(Json(json!({
        "success": true,
        "message": "Новый код подтверждения отправлен на email",
        "expires_in": 900
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

pub async fn register(
    State(state): State<AppState>,
    Json(payload): Json<RegisterRequest>,
) -> Result<Json<AuthResponse>, (StatusCode, Json<Value>)> {
    if let Err(err) = validate_username(&payload.username) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": err }))));
    }
    let clean_username = payload.username.trim().to_string();

    if let Err(err) = validate_password(&payload.password) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": err }))));
    }

    let clean_email = match &payload.email {
        Some(e) if !e.trim().is_empty() => match validate_email(e) {
            Ok(valid) => Some(valid),
            Err(err) => return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": err })))),
        },
        _ => None,
    };

    let exists = sqlx::query("SELECT id FROM users WHERE LOWER(username) = LOWER(?)")
        .bind(&clean_username)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Ошибка базы данных" })),
            )
        })?;

    if exists.is_some() {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({ "error": "Пользователь с таким логином уже существует" })),
        ));
    }

    if let Some(ref email) = clean_email {
        let email_exists = sqlx::query("SELECT id FROM users WHERE LOWER(email) = LOWER(?)")
            .bind(email)
            .fetch_optional(&state.pool)
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "Ошибка базы данных" })),
                )
            })?;

        if email_exists.is_some() {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({ "error": "Пользователь с такой почтой уже существует" })),
            ));
        }
    }

    let password_hash = hash_password(payload.password.trim()).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Не удалось захэшировать пароль" })),
        )
    })?;

    let user_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO users (id, username, email, password_hash, created_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&user_id)
    .bind(&clean_username)
    .bind(&clean_email)
    .bind(&password_hash)
    .bind(now)
    .execute(&state.pool)
    .await
    .map_err(|err| {
        eprintln!("[SIGNAL AUTH] Ошибка регистрации пользователя: {}", err);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Не удалось зарегистрировать пользователя" })),
        )
    })?;

    let token = create_jwt(&user_id, &clean_username, clean_email.as_deref()).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Не удалось сгенерировать токен авторизации" })),
        )
    })?;

    Ok(Json(AuthResponse {
        token,
        user: UserInfo {
            id: user_id,
            username: clean_username,
            email: clean_email,
        },
    }))
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, (StatusCode, Json<Value>)> {
    // Rate-limit: не более 10 попыток за 5 минут с одного IP+логина
    let rate_key = {
        let ip = headers
            .get("x-forwarded-for")
            .or_else(|| headers.get("x-real-ip"))
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .split(',')
            .next()
            .unwrap_or("unknown")
            .trim()
            .to_string();
        format!("{}:{}", ip, payload.login.trim().to_lowercase())
    };

    {
        let mut attempts = state.login_attempts.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let window = std::time::Duration::from_secs(300); // 5 минут
        let max_attempts = 10u32;

        let entry = attempts.entry(rate_key.clone()).or_insert((0, now));
        if now.duration_since(entry.1) > window {
            *entry = (1, now); // сброс окна
        } else {
            entry.0 += 1;
            if entry.0 > max_attempts {
                return Err((
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(json!({ "error": "Слишком много попыток входа. Подождите 5 минут." })),
                ));
            }
        }
    }

    let login = payload.login.trim();
    let password = payload.password.trim();

    if login.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Введите логин или email" })),
        ));
    }

    if password.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Введите пароль" })),
        ));
    }

    let row = sqlx::query("SELECT id, username, email, password_hash FROM users WHERE LOWER(username) = LOWER(?) OR LOWER(email) = LOWER(?)")
        .bind(login)
        .bind(login)
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

    // Успешный вход — сбрасываем счётчик попыток
    {
        let mut attempts = state.login_attempts.lock().unwrap_or_else(|e| e.into_inner());
        attempts.remove(&rate_key);
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

pub async fn request_password_reset(
    State(state): State<AppState>,
    Json(payload): Json<ResetPasswordRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let clean_input = payload.email_or_login.trim().to_lowercase();
    if clean_input.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Введите логин или email" })),
        ));
    }

    let user_row = sqlx::query("SELECT id, username, email FROM users WHERE LOWER(username) = LOWER(?) OR LOWER(email) = LOWER(?)")
        .bind(&clean_input)
        .bind(&clean_input)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Ошибка базы данных" })),
            )
        })?;

    let row = match user_row {
        Some(r) => r,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Пользователь с таким логином или email не найден" })),
            ));
        }
    };

    let user_id: String = row.get("id");
    let email: Option<String> = row.get("email");
    let target_email = match email {
        Some(e) if !e.trim().is_empty() => e,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "К этому аккаунту не привязан email для сброса пароля" })),
            ));
        }
    };

    let code = format!("{:06}", fastrand::u32(100000..=999999));
    let now = chrono::Utc::now().timestamp();
    let expires_at = now + 900;

    sqlx::query(
        r#"
        INSERT INTO password_resets (email, code, user_id, expires_at, created_at, attempts)
        VALUES (?, ?, ?, ?, ?, 0)
        ON CONFLICT(email) DO UPDATE SET
            code = excluded.code,
            user_id = excluded.user_id,
            expires_at = excluded.expires_at,
            created_at = excluded.created_at,
            attempts = 0
        "#,
    )
    .bind(&target_email)
    .bind(&code)
    .bind(&user_id)
    .bind(expires_at)
    .bind(now)
    .execute(&state.pool)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Не удалось создать код сброса" })),
        )
    })?;

    send_verification_email(&target_email, &code).await.map_err(|err_msg| {
        eprintln!("[SIGNAL AUTH] Ошибка отправки кода сброса на {}: {}", target_email, err_msg);
        (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "Не удалось отправить письмо с кодом сброса. Попробуйте позже." })),
        )
    })?;

    Ok(Json(json!({
        "success": true,
        "email": target_email,
        "message": "Код для сброса пароля отправлен на вашу почту",
        "expires_in": 900
    })))
}

pub async fn confirm_password_reset(
    State(state): State<AppState>,
    Json(payload): Json<ConfirmResetPasswordRequest>,
) -> Result<Json<AuthResponse>, (StatusCode, Json<Value>)> {
    let clean_input = payload.email_or_login.trim().to_lowercase();
    let code = payload.code.trim();
    let new_password = payload.new_password.trim();

    if let Err(err) = validate_password(new_password) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": err }))));
    }

    let reset_record = sqlx::query(
        r#"
        SELECT pr.email, pr.code, pr.user_id, pr.expires_at, pr.attempts, u.username, u.email as u_email
        FROM password_resets pr
        JOIN users u ON u.id = pr.user_id
        WHERE LOWER(pr.email) = LOWER(?) OR LOWER(u.username) = LOWER(?)
        "#
    )
    .bind(&clean_input)
    .bind(&clean_input)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Ошибка базы данных" })),
        )
    })?;

    let row = match reset_record {
        Some(r) => r,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Код сброса пароля не найден или устарел. Запросите сброс заново." })),
            ));
        }
    };

    let db_email: String = row.get("email");
    let db_code: String = row.get("code");
    let user_id: String = row.get("user_id");
    let username: String = row.get("username");
    let u_email: Option<String> = row.get("u_email");
    let expires_at: i64 = row.get("expires_at");
    let attempts: i64 = row.get("attempts");

    let now = chrono::Utc::now().timestamp();
    if now > expires_at {
        let _ = sqlx::query("DELETE FROM password_resets WHERE email = ?").bind(&db_email).execute(&state.pool).await;
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Срок действия кода истек. Запросите новый код." })),
        ));
    }

    if attempts >= 5 {
        let _ = sqlx::query("DELETE FROM password_resets WHERE email = ?").bind(&db_email).execute(&state.pool).await;
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "Превышено число попыток ввода. Запросите сброс заново." })),
        ));
    }

    if db_code != code {
        let _ = sqlx::query("UPDATE password_resets SET attempts = attempts + 1 WHERE email = ?").bind(&db_email).execute(&state.pool).await;
        let left = 4 - attempts;
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": format!("Неверный код подтверждения. Осталось попыток: {}", if left > 0 { left } else { 0 })
            })),
        ));
    }

    let password_hash = hash_password(new_password).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Не удалось обновить пароль" })),
        )
    })?;

    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(&password_hash)
        .bind(&user_id)
        .execute(&state.pool)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Не удалось сохранить новый пароль" })),
            )
        })?;

    let _ = sqlx::query("DELETE FROM password_resets WHERE email = ?").bind(&db_email).execute(&state.pool).await;

    let token = create_jwt(&user_id, &username, u_email.as_deref()).map_err(|_| {
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
            email: u_email,
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
