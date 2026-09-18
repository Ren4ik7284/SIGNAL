mod config;
mod handlers;
mod models;
mod ytdlp;

use axum::routing::get;
use axum::Router;
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;

use config::ensure_cookies_on_start;
use handlers::cover::{health_check, proxy_cover};
use handlers::library::{get_library, save_library};
use handlers::search::{extract_info, search_music};
use handlers::stream::stream_audio;

#[tokio::main]
async fn main() {
    ensure_cookies_on_start().await;

    let cors = CorsLayer::permissive();

    let app = Router::new()
        .route("/", get(|| async { "SIGNAL // Audio Backend is running" }))
        .route("/api/health", get(health_check))
        .route("/api/cover", get(proxy_cover))
        .route("/api/search", get(search_music))
        .route("/api/extract", get(extract_info))
        .route("/api/stream", get(stream_audio))
        .route("/api/sync", get(get_library).post(save_library))
        .layer(cors);

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
