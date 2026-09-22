mod auth;
mod config;
mod db;
mod email;
mod handlers;
mod models;
mod ytdlp;

use axum::routing::{get, post};
use axum::Router;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tower_http::cors::{Any, CorsLayer};
use tower_http::limit::RequestBodyLimitLayer;

use config::{ensure_cookies_on_start, init_cookies_from_env};
use db::{init_db, DbPool};
use handlers::auth::{
    confirm_password_reset, get_me, login, register, request_password_reset, resend_verification_code,
    send_verification_code, verify_registration_code,
};
use handlers::cover::{health_check, proxy_cover};
use handlers::history::{clear_history, get_history, record_play};
use handlers::library::{get_library, save_library};
use handlers::search::{extract_info, search_music};
use handlers::stats::get_wrapped;
use handlers::stream::stream_audio;

/// Состояние rate-limiting для эндпоинта логина.
/// Ключ: "IP:login", значение: (кол-во попыток, время первой попытки в окне)
pub type LoginAttempts = Arc<Mutex<HashMap<String, (u32, Instant)>>>;

#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub login_attempts: LoginAttempts,
}

#[tokio::main]
async fn main() {
    // Предупреждение если JWT секрет не задан
    if std::env::var("JWT_SECRET").is_err() {
        eprintln!("[SIGNAL WARN] JWT_SECRET не задан! Используется дефолтный ключ — НЕБЕЗОПАСНО для продакшена. Задайте переменную окружения JWT_SECRET.");
    }

    // Initialize cookies from env variable immediately
    init_cookies_from_env();

    // Refresh cookies in background — don't block server startup
    tokio::spawn(ensure_cookies_on_start());

    let pool = match init_db().await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Failed to initialize database: {}", e);
            return;
        }
    };

    let state = AppState {
        pool,
        login_attempts: Arc::new(Mutex::new(HashMap::new())),
    };

    // CORS: разрешаем любые origins (self-hosted), но без credentials cookie
    // Для продакшена с фиксированным доменом задайте ALLOWED_ORIGINS=https://yourdomain.com
    let cors = if let Ok(origins_str) = std::env::var("ALLOWED_ORIGINS") {
        let allowed: Vec<axum::http::HeaderValue> = origins_str
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        if allowed.is_empty() {
            CorsLayer::permissive()
        } else {
            CorsLayer::new()
                .allow_origin(allowed)
                .allow_headers(Any)
                .allow_methods(Any)
        }
    } else {
        CorsLayer::permissive()
    };

    let app = Router::new()
        .route("/", get(|| async { "SIGNAL // Audio Backend is running" }))
        .route("/api/health", get(health_check))
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/send-code", post(send_verification_code))
        .route("/api/auth/verify-code", post(verify_registration_code))
        .route("/api/auth/resend-code", post(resend_verification_code))
        .route("/api/auth/reset-password-code", post(request_password_reset))
        .route("/api/auth/reset-password", post(confirm_password_reset))
        .route("/api/auth/me", get(get_me))
        .route("/api/sync", get(get_library).post(save_library))
        .route("/api/history", get(get_history).post(record_play).delete(clear_history))
        .route("/api/stats/wrapped", get(get_wrapped))
        .route("/api/cover", get(proxy_cover))
        .route("/api/search", get(search_music))
        .route("/api/extract", get(extract_info))
        .route("/api/stream", get(stream_audio))
        // Лимит тела запроса: 5 МБ для /api/sync, защита от огромных payload
        .layer(RequestBodyLimitLayer::new(5 * 1024 * 1024))
        .layer(cors)
        .with_state(state);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8085);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("Server running on port {}", port);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind port {}: {}", port, e);
            return;
        }
    };

    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("Server error: {}", e);
    }
}
