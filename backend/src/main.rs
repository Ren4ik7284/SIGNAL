mod auth;
mod config;
mod db;
mod handlers;
mod models;
mod security;
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
use handlers::auth::{get_auth_config, get_me, google_login, login, register};
use handlers::cover::{health_check, proxy_cover};
use handlers::history::{clear_history, get_history, record_play};
use handlers::library::{get_library, save_library};
use handlers::search::{extract_info, search_music};
use handlers::stats::get_wrapped;
use handlers::stream::stream_audio;

pub type LoginAttempts = Arc<Mutex<HashMap<String, (u32, Instant)>>>;
pub type EndpointRateLimits = Arc<Mutex<HashMap<String, (u32, Instant)>>>;
pub type StreamCache = Arc<Mutex<HashMap<String, (String, Instant)>>>;

#[derive(Clone)]
pub struct AppState {
    pub pool: DbPool,
    pub login_attempts: LoginAttempts,
    pub endpoint_rate_limits: EndpointRateLimits,
    pub heavy_process_semaphore: Arc<tokio::sync::Semaphore>,
    pub stream_cache: StreamCache,
}

#[tokio::main]
async fn main() {
    init_cookies_from_env();
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
        endpoint_rate_limits: Arc::new(Mutex::new(HashMap::new())),
        heavy_process_semaphore: Arc::new(tokio::sync::Semaphore::new(16)),
        stream_cache: Arc::new(Mutex::new(HashMap::new())),
    };

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
        .route("/", get(|| async { "Recro // Audio Backend is running" }))
        .route("/api/health", get(health_check))
        .route("/api/auth/register", post(register))
        .route("/api/auth/login", post(login))
        .route("/api/auth/me", get(get_me))
        .route("/api/auth/config", get(get_auth_config))
        .route("/api/auth/google", post(google_login))
        .route("/api/sync", get(get_library).post(save_library))
        .route("/api/history", get(get_history).post(record_play).delete(clear_history))
        .route("/api/stats/wrapped", get(get_wrapped))
        .route("/api/cover", get(proxy_cover))
        .route("/api/search", get(search_music))
        .route("/api/extract", get(extract_info))
        .route("/api/stream", get(stream_audio))
        .layer(RequestBodyLimitLayer::new(5 * 1024 * 1024))
        .layer(cors)
        .with_state(state);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8085);

    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let ip: std::net::IpAddr = host.parse().unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
    let addr = SocketAddr::from((ip, port));
    println!("Server running on http://{}:{}", ip, port);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to bind port {}: {}", port, e);
            return;
        }
    };

    if let Err(e) = axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).await {
        eprintln!("Server error: {}", e);
    }
}
