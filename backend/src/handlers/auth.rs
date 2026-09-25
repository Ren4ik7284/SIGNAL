use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::auth::{
    create_jwt, extract_claims_from_headers, hash_password, validate_password,
    validate_username, verify_password, AuthConfigResponse, AuthResponse, GoogleAuthRequest,
    LoginRequest, RegisterRequest, UserInfo,
};
use crate::security::get_client_ip;
use crate::AppState;

pub async fn register(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<RegisterRequest>,
) -> Result<Json<AuthResponse>, (StatusCode, Json<Value>)> {
    let ip = get_client_ip(&headers, Some(addr));

    {
        let mut attempts = state.login_attempts.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let window = Duration::from_secs(600);

        if attempts.len() > 2000 {
            attempts.retain(|_, (_, start)| now.duration_since(*start) <= window);
        }

        let reg_entry = attempts.entry(format!("reg:{}", ip)).or_insert((0, now));
        if now.duration_since(reg_entry.1) > window {
            *reg_entry = (1, now);
        } else {
            reg_entry.0 += 1;
            if reg_entry.0 > 5 {
                return Err((
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(json!({ "error": "Слишком много попыток регистрации. Подождите 10 минут." })),
                ));
            }
        }
    }

    if let Err(err) = validate_username(&payload.username) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": err }))));
    }
    let clean_username = payload.username.trim().to_string();

    if let Err(err) = validate_password(&payload.password) {
        return Err((StatusCode::BAD_REQUEST, Json(json!({ "error": err }))));
    }

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

    let password_hash = hash_password(payload.password.trim()).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Не удалось захэшировать пароль" })),
        )
    })?;

    let user_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO users (id, username, password_hash, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(&user_id)
    .bind(&clean_username)
    .bind(&password_hash)
    .bind(now)
    .execute(&state.pool)
    .await
    .map_err(|err| {
        eprintln!("Registration DB error: {}", err);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Не удалось зарегистрировать пользователя" })),
        )
    })?;

    let token = create_jwt(&user_id, &clean_username).map_err(|_| {
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
            email: None,
            avatar_url: None,
        },
    }))
}

pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, (StatusCode, Json<Value>)> {
    let clean_login = payload.login.trim().to_lowercase();
    let ip = get_client_ip(&headers, Some(addr));
    let acct_ip_key = format!("acct_ip:{}:{}", clean_login, ip);
    let ip_key = format!("ip:{}", ip);

    {
        let mut attempts = state.login_attempts.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let window = Duration::from_secs(300);

        if attempts.len() > 2000 {
            attempts.retain(|_, (_, start)| now.duration_since(*start) <= window);
        }

        if !clean_login.is_empty() {
            if let Some(acct_entry) = attempts.get(&acct_ip_key) {
                if now.duration_since(acct_entry.1) <= window && acct_entry.0 >= 5 {
                    return Err((
                        StatusCode::TOO_MANY_REQUESTS,
                        Json(json!({ "error": "Слишком много неудачных попыток входа для этой связки логина и IP. Подождите 5 минут." })),
                    ));
                }
            }
        }

        if let Some(ip_entry) = attempts.get(&ip_key) {
            if now.duration_since(ip_entry.1) <= window && ip_entry.0 >= 20 {
                return Err((
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(json!({ "error": "Слишком много неудачных запросов с вашего IP. Подождите 5 минут." })),
                ));
            }
        }
    }

    let login = payload.login.trim();
    let password = payload.password.trim();

    if login.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Введите имя пользователя" })),
        ));
    }

    if password.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Введите пароль" })),
        ));
    }

    let row = sqlx::query("SELECT id, username, password_hash, email, avatar_url FROM users WHERE LOWER(username) = LOWER(?)")
        .bind(login)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Ошибка базы данных" })),
            )
        })?;

    let record_attempt = || {
        let mut attempts = state.login_attempts.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let window = Duration::from_secs(300);

        if !clean_login.is_empty() {
            let entry = attempts.entry(acct_ip_key.clone()).or_insert((0, now));
            if now.duration_since(entry.1) > window {
                *entry = (1, now);
            } else {
                entry.0 += 1;
            }
        }

        let ip_entry = attempts.entry(ip_key.clone()).or_insert((0, now));
        if now.duration_since(ip_entry.1) > window {
            *ip_entry = (1, now);
        } else {
            ip_entry.0 += 1;
        }
    };

    let user_row = match row {
        Some(r) => r,
        None => {
            record_attempt();
            tokio::time::sleep(Duration::from_millis(250)).await;
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "Неверное имя пользователя или пароль" })),
            ));
        }
    };

    let user_id: String = user_row.get("id");
    let db_username: String = user_row.get("username");
    let password_hash: String = user_row.get("password_hash");

    if !verify_password(password, &password_hash) {
        record_attempt();
        tokio::time::sleep(Duration::from_millis(250)).await;
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Неверное имя пользователя или пароль" })),
        ));
    }

    {
        let mut attempts = state.login_attempts.lock().unwrap_or_else(|e| e.into_inner());
        attempts.remove(&acct_ip_key);
    }

    let token = create_jwt(&user_id, &db_username).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Не удалось сгенерировать токен авторизации" })),
        )
    })?;

    let user_email: Option<String> = user_row.get("email");
    let user_avatar: Option<String> = user_row.get("avatar_url");

    Ok(Json(AuthResponse {
        token,
        user: UserInfo {
            id: user_id,
            username: db_username,
            email: user_email,
            avatar_url: user_avatar,
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

    let row = sqlx::query("SELECT id, username, email, avatar_url FROM users WHERE id = ?")
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
            let avatar_url: Option<String> = r.get("avatar_url");
            Ok(Json(UserInfo { id, username, email, avatar_url }))
        }
        None => Err((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Пользователь не найден" })),
        )),
    }
}

pub async fn get_auth_config() -> Json<AuthConfigResponse> {
    let client_id = std::env::var("GOOGLE_CLIENT_ID")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let enabled = client_id.is_some();
    Json(AuthConfigResponse {
        google_client_id: client_id,
        google_auth_enabled: enabled,
    })
}

#[derive(Debug, Deserialize)]
struct GoogleTokenInfo {
    iss: String,
    sub: String,
    aud: String,
    email: Option<String>,
    #[serde(default)]
    email_verified: Option<Value>,
    name: Option<String>,
    picture: Option<String>,
    exp: Option<Value>,
}

pub async fn google_login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(payload): Json<GoogleAuthRequest>,
) -> Result<Json<AuthResponse>, (StatusCode, Json<Value>)> {
    let ip = get_client_ip(&headers, Some(addr));

    {
        let mut attempts = state.login_attempts.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        let window = Duration::from_secs(300);

        if attempts.len() > 2000 {
            attempts.retain(|_, (_, start)| now.duration_since(*start) <= window);
        }

        let ip_entry = attempts.entry(format!("g_ip:{}", ip)).or_insert((0, now));
        if now.duration_since(ip_entry.1) > window {
            *ip_entry = (1, now);
        } else {
            ip_entry.0 += 1;
            if ip_entry.0 > 20 {
                return Err((
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(json!({ "error": "Слишком много попыток входа через Google. Подождите 5 минут." })),
                ));
            }
        }
    }

    let expected_client_id = match std::env::var("GOOGLE_CLIENT_ID") {
        Ok(val) if !val.trim().is_empty() => val.trim().to_string(),
        _ => {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "Google авторизация отключена или не настроена на сервере" })),
            ));
        }
    };

    let cred = payload.credential.trim();
    if cred.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Отсутствует Google токен (credential)" })),
        ));
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Ошибка инициализации HTTP-клиента" })),
            )
        })?;

    let resp = client
        .get("https://oauth2.googleapis.com/tokeninfo")
        .query(&[("id_token", cred)])
        .send()
        .await
        .map_err(|e| {
            eprintln!("Google auth network error: {}", e);
            (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": "Не удалось связаться с серверами Google для проверки подлинности" })),
            )
        })?;

    if !resp.status().is_success() {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Недействительный или просроченный токен Google" })),
        ));
    }

    let info: GoogleTokenInfo = resp.json().await.map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Ошибка обработки ответа Google" })),
        )
    })?;

    if info.iss != "https://accounts.google.com" && info.iss != "accounts.google.com" {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Неверный издатель токена Google (iss)" })),
        ));
    }

    if info.aud != expected_client_id {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Токен выдан для другого Client ID" })),
        ));
    }

    let exp_timestamp = match &info.exp {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.parse::<i64>().ok(),
        _ => None,
    };
    if let Some(exp) = exp_timestamp {
        if chrono::Utc::now().timestamp() >= exp {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "Срок действия токена Google истёк" })),
            ));
        }
    }

    let email_verified = match &info.email_verified {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s.eq_ignore_ascii_case("true"),
        _ => false,
    };
    if !email_verified {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "Email вашего Google-аккаунта не подтверждён" })),
        ));
    }

    let google_id = info.sub.trim().to_string();
    let email_opt = info.email.as_ref().map(|e| e.trim().to_lowercase()).filter(|e| !e.is_empty());
    let avatar_opt = info.picture.as_ref().map(|p| p.trim().to_string()).filter(|p| !p.is_empty());

    let user_by_google = sqlx::query("SELECT id, username, email, avatar_url FROM users WHERE google_id = ?")
        .bind(&google_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|_| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "Ошибка базы данных" })))
        })?;

    if let Some(row) = user_by_google {
        let user_id: String = row.get("id");
        let username: String = row.get("username");
        let current_email: Option<String> = row.get("email");
        let current_avatar: Option<String> = row.get("avatar_url");

        if (avatar_opt.is_some() && current_avatar != avatar_opt) || (email_opt.is_some() && current_email != email_opt) {
            let _ = sqlx::query("UPDATE users SET avatar_url = COALESCE(?, avatar_url), email = COALESCE(?, email) WHERE id = ?")
                .bind(&avatar_opt)
                .bind(&email_opt)
                .bind(&user_id)
                .execute(&state.pool)
                .await;
        }

        let token = create_jwt(&user_id, &username).map_err(|_| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "Ошибка генерации токена" })))
        })?;

        return Ok(Json(AuthResponse {
            token,
            user: UserInfo {
                id: user_id,
                username,
                email: email_opt.or(current_email),
                avatar_url: avatar_opt.or(current_avatar),
            },
        }));
    }

    if let Some(ref email) = email_opt {
        let user_by_email = sqlx::query("SELECT id, username, email, avatar_url, google_id FROM users WHERE LOWER(email) = LOWER(?)")
            .bind(email)
            .fetch_optional(&state.pool)
            .await
            .map_err(|_| {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "Ошибка базы данных" })))
            })?;

        if let Some(row) = user_by_email {
            let existing_google_id: Option<String> = row.get("google_id");
            if let Some(ref gid) = existing_google_id {
                if gid != &google_id {
                    return Err((
                        StatusCode::CONFLICT,
                        Json(json!({ "error": "Этот email уже привязан к другому Google-аккаунту" })),
                    ));
                }
            }

            let user_id: String = row.get("id");
            let username: String = row.get("username");
            let current_avatar: Option<String> = row.get("avatar_url");

            let _ = sqlx::query("UPDATE users SET google_id = ?, avatar_url = COALESCE(avatar_url, ?) WHERE id = ?")
                .bind(&google_id)
                .bind(&avatar_opt)
                .bind(&user_id)
                .execute(&state.pool)
                .await;

            let token = create_jwt(&user_id, &username).map_err(|_| {
                (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "Ошибка генерации токена" })))
            })?;

            return Ok(Json(AuthResponse {
                token,
                user: UserInfo {
                    id: user_id,
                    username,
                    email: Some(email.clone()),
                    avatar_url: avatar_opt.or(current_avatar),
                },
            }));
        }
    }

    let base_name = if let Some(ref name) = info.name {
        let cleaned: String = name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
            .collect();
        let trimmed = cleaned.trim_matches('_');
        if trimmed.len() >= 3 {
            trimmed[..trimmed.len().min(20)].to_string()
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let base_name = if base_name.is_empty() {
        if let Some(ref email) = email_opt {
            let prefix = email.split('@').next().unwrap_or("user");
            let cleaned: String = prefix
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
                .collect();
            let trimmed = cleaned.trim_matches('_');
            if trimmed.len() >= 3 {
                trimmed[..trimmed.len().min(20)].to_string()
            } else {
                format!("user_{}", &google_id[..6.min(google_id.len())])
            }
        } else {
            format!("user_{}", &google_id[..6.min(google_id.len())])
        }
    } else {
        base_name
    };

    let mut chosen_username = base_name.clone();
    let mut counter = 1;
    loop {
        let exists = sqlx::query("SELECT id FROM users WHERE LOWER(username) = LOWER(?)")
            .bind(&chosen_username)
            .fetch_optional(&state.pool)
            .await
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "Ошибка базы данных" }))))?;

        if exists.is_none() {
            break;
        }

        chosen_username = format!("{}_{}", &base_name[..base_name.len().min(16)], counter);
        counter += 1;
        if counter > 100 {
            chosen_username = format!("user_{}", &Uuid::new_v4().to_string()[..8]);
            break;
        }
    }

    let random_pass = format!("goog_{}_{}", Uuid::new_v4(), fastrand::u64(..));
    let password_hash = hash_password(&random_pass).map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "Не удалось сгенерировать хэш" })))
    })?;

    let user_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO users (id, username, password_hash, google_id, email, avatar_url, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&user_id)
    .bind(&chosen_username)
    .bind(&password_hash)
    .bind(&google_id)
    .bind(&email_opt)
    .bind(&avatar_opt)
    .bind(now)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        eprintln!("Google auth user creation error: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "Не удалось создать аккаунт Google" })))
    })?;

    let token = create_jwt(&user_id, &chosen_username).map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "Не удалось сгенерировать токен" })))
    })?;

    Ok(Json(AuthResponse {
        token,
        user: UserInfo {
            id: user_id,
            username: chosen_username,
            email: email_opt,
            avatar_url: avatar_opt,
        },
    }))
}
