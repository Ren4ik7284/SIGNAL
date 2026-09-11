use axum::{
    body::Body,
    extract::Query,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::Path;
use std::process::Stdio;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tokio_util::io::ReaderStream;
use tower_http::cors::CorsLayer;

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub q: String,
}

#[derive(Debug, Deserialize)]
pub struct StreamParams {
    pub url: Option<String>,
    pub id: Option<String>,
    pub ss: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub duration: f64,
    pub audio_url: String,
    pub cover_url: Option<String>,
}

fn get_yt_dlp_cmd() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let local_path = format!("{}/.local/bin/yt-dlp", home);
    if Path::new(&local_path).exists() {
        local_path
    } else {
        "yt-dlp".to_string()
    }
}

async fn health_check() -> &'static str {
    "SIGNAL // Rust Engine Online"
}

async fn search_music(Query(params): Query<SearchParams>) -> Result<Json<Vec<SearchTrack>>, StatusCode> {
    let query = params.q.trim();
    if query.is_empty() {
        return Ok(Json(Vec::new()));
    }

    println!("[SEARCH] Поисковый запрос: \"{}\"", query);

    let yt_cmd = get_yt_dlp_cmd();

    let mut child = Command::new(&yt_cmd)
        .args([
            &format!("scsearch10:{}", query),
            "--dump-json",
            "--flat-playlist",
            "--no-warnings",
            "--no-check-certificates",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| {
            eprintln!("[ERROR] Не удалось запустить yt-dlp: {}", err);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let stdout = child.stdout.take().ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut reader = tokio::io::BufReader::new(stdout).lines();
    let mut tracks = Vec::new();

    while let Ok(Some(line)) = reader.next_line().await {
        if let Ok(item) = serde_json::from_str::<serde_json::Value>(&line) {
            let id = item["id"].as_str().unwrap_or("").to_string();
            let title = item["title"].as_str().unwrap_or("Без названия").to_string();
            let artist = item["uploader"].as_str().unwrap_or("Неизвестный исполнитель").to_string();
            let duration = item["duration"].as_f64().unwrap_or(0.0);

            let track_url = item["webpage_url"]
                .as_str()
                .or_else(|| item["url"].as_str())
                .unwrap_or("")
                .to_string();

            let mut cover_url = item["thumbnails"]
                .as_array()
                .and_then(|thumbs| thumbs.first())
                .and_then(|t| t["url"].as_str())
                .map(|s| s.to_string());

            if let Some(ref c) = cover_url {
                if c.contains("-mini.") {
                    cover_url = Some(c.replace("-mini.", "-t500x500."));
                }
            }

            if !id.is_empty() {
                let base_url = std::env::var("RAILWAY_PUBLIC_DOMAIN")
                    .map(|domain| format!("https://{}", domain))
                    .unwrap_or_else(|_| "http://localhost:8085".to_string());

                let audio_url = if !track_url.is_empty() {
                    let encoded_url = urlencoding::encode(&track_url);
                    format!("{}/api/stream?url={}", base_url, encoded_url)
                } else {
                    format!("{}/api/stream?id={}", base_url, id)
                };

                tracks.push(SearchTrack {
                    id,
                    title,
                    artist,
                    duration,
                    audio_url,
                    cover_url,
                });
            }
        }
    }

    let _ = child.wait().await;
    println!("[SEARCH] Успешно найдено треков: {}", tracks.len());

    Ok(Json(tracks))
}

async fn stream_audio(
    Query(params): Query<StreamParams>,
    _client_headers: HeaderMap,
) -> Result<Response, StatusCode> {
    let target = match (params.url, params.id) {
        (Some(u), _) if !u.trim().is_empty() => u.trim().to_string(),
        (_, Some(id)) if !id.trim().is_empty() => {
            if id.starts_with("http") {
                id.trim().to_string()
            } else {
                format!("https://api.soundcloud.com/tracks/{}", id.trim())
            }
        }
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    println!("[STREAM] Запрос на стриминг источника: {}", target);

    let yt_cmd = get_yt_dlp_cmd();

    let output = Command::new(&yt_cmd)
        .args([
            "-g",
            "-f",
            "ba/b",
            "--no-warnings",
            "--no-check-certificates",
            &target,
        ])
        .output()
        .await
        .map_err(|e| {
            eprintln!("[ERROR] Ошибка запуска yt-dlp -g: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if !output.status.success() {
        eprintln!("[ERROR] yt-dlp не смог получить ссылку на аудиопоток");
        return Err(StatusCode::NOT_FOUND);
    }

    let direct_url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if direct_url.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    let mut ffmpeg_args = vec![
        "-reconnect".to_string(),
        "1".to_string(),
        "-reconnect_streamed".to_string(),
        "1".to_string(),
        "-reconnect_delay_max".to_string(),
        "5".to_string(),
    ];

    if let Some(seek_sec) = params.ss {
        if seek_sec > 0 {
            ffmpeg_args.push("-ss".to_string());
            ffmpeg_args.push(seek_sec.to_string());
        }
    }

    ffmpeg_args.extend([
        "-i".to_string(),
        direct_url,
        "-vn".to_string(),
        "-f".to_string(),
        "mp3".to_string(),
        "-b:a".to_string(),
        "192k".to_string(),
        "pipe:1".to_string(),
    ]);

    let mut ffmpeg_child = Command::new("ffmpeg")
        .args(&ffmpeg_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            eprintln!("[ERROR] Не удалось запустить FFmpeg: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let stdout = ffmpeg_child
        .stdout
        .take()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let stream = ReaderStream::new(stdout);
    let body = Body::from_stream(stream);

    let mut res_headers = HeaderMap::new();
    res_headers.insert(header::CONTENT_TYPE, "audio/mpeg".parse().unwrap());
    res_headers.insert(header::CACHE_CONTROL, "no-cache, no-store".parse().unwrap());
    res_headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap());
    res_headers.insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, "*".parse().unwrap());

    println!("[STREAM] Аудиопоток успешно открыт и передается в браузер");

    Ok((StatusCode::OK, res_headers, body).into_response())
}

#[tokio::main]
async fn main() {
    println!("--------------------------------------------------");
    println!("  SIGNAL // Minimalist Audio Backend (Rust Axum)  ");
    println!("--------------------------------------------------");

    let cors = CorsLayer::permissive();

    let app = Router::new()
        .route("/", get(|| async { "SIGNAL // Audio Backend is running. Use /api/health, /api/search, /api/stream" }))
        .route("/api/health", get(health_check))
        .route("/api/search", get(search_music))
        .route("/api/stream", get(stream_audio))
        .layer(cors);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8085);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("[ONLINE] Сервер запущен: http://localhost:{}", port);
    println!("[ROUTES] GET /api/health     -> Проверка доступности");
    println!("[ROUTES] GET /api/search?q=..-> Поиск музыки в реальном времени");
    println!("[ROUTES] GET /api/stream?url=-> Прямой стриминг аудиопотока");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Не удалось занять порт 8085. Проверьте, не занят ли порт другим процессом.");

    axum::serve(listener, app)
        .await
        .expect("Критическая ошибка работы сервера Axum");
}
