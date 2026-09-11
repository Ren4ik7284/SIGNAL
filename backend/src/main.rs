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
pub struct ExtractParams {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct StreamParams {
    pub url: Option<String>,
    pub id: Option<String>,
    pub ss: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub duration: f64,
    pub audio_url: String,
    pub cover_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExtractResponse {
    pub playlist_title: Option<String>,
    pub tracks: Vec<SearchTrack>,
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

fn get_base_url() -> String {
    if let Ok(domain) = std::env::var("RAILWAY_PUBLIC_DOMAIN") {
        return format!("https://{}", domain);
    }
    let port = std::env::var("PORT").unwrap_or_else(|_| "8085".to_string());
    format!("http://localhost:{}", port)
}

fn parse_track_json(item: &serde_json::Value, base_url: &str) -> Option<SearchTrack> {
    let id = match item["id"].as_str() {
        Some(val) => val.to_string(),
        None => return None,
    };

    if id.is_empty() {
        return None;
    }

    let mut title = "Без названия".to_string();
    if let Some(t) = item["title"].as_str() {
        title = t.to_string();
    }

    let mut artist = "Неизвестный исполнитель".to_string();
    if let Some(u) = item["uploader"].as_str() {
        artist = u.to_string();
    } else if let Some(c) = item["channel"].as_str() {
        artist = c.to_string();
    } else if let Some(a) = item["artist"].as_str() {
        artist = a.to_string();
    }

    let duration = item["duration"].as_f64().unwrap_or(0.0);

    let track_url = if let Some(u) = item["webpage_url"].as_str() {
        u.to_string()
    } else if let Some(u) = item["url"].as_str() {
        if u.starts_with("http") {
            u.to_string()
        } else {
            format!("https://www.youtube.com/watch?v={}", id)
        }
    } else {
        format!("https://www.youtube.com/watch?v={}", id)
    };

    let mut cover_url = None;
    if let Some(thumbs) = item["thumbnails"].as_array() {
        if let Some(last) = thumbs.last() {
            if let Some(u) = last["url"].as_str() {
                cover_url = Some(u.to_string());
            }
        }
    }
    if cover_url.is_none() {
        if let Some(u) = item["thumbnail"].as_str() {
            cover_url = Some(u.to_string());
        }
    }

    let encoded_url = urlencoding::encode(&track_url);
    let audio_url = format!("{}/api/stream?url={}", base_url, encoded_url);

    Some(SearchTrack {
        id,
        title,
        artist,
        duration,
        audio_url,
        cover_url,
    })
}

async fn health_check() -> &'static str {
    "SIGNAL // Rust Engine Online"
}

async fn search_music(Query(params): Query<SearchParams>) -> Result<Json<Vec<SearchTrack>>, StatusCode> {
    let query = params.q.trim();
    if query.is_empty() {
        return Ok(Json(Vec::new()));
    }

    let yt_cmd = get_yt_dlp_cmd();
    let base_url = get_base_url();

    let search_arg = if query.starts_with("http://") || query.starts_with("https://") {
        query.to_string()
    } else {
        format!("ytsearch10:{}", query)
    };

    let mut child = match Command::new(&yt_cmd)
        .args([
            &search_arg,
            "--dump-json",
            "--flat-playlist",
            "--no-warnings",
            "--no-check-certificates",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let mut reader = tokio::io::BufReader::new(stdout).lines();
    let mut tracks = Vec::new();

    while let Ok(Some(line)) = reader.next_line().await {
        if let Ok(item) = serde_json::from_str::<serde_json::Value>(&line) {
            if let Some(track) = parse_track_json(&item, &base_url) {
                tracks.push(track);
            }
        }
    }

    let _ = child.wait().await;

    Ok(Json(tracks))
}

async fn extract_info(Query(params): Query<ExtractParams>) -> Result<Json<ExtractResponse>, StatusCode> {
    let url = params.url.trim();
    if url.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let yt_cmd = get_yt_dlp_cmd();
    let base_url = get_base_url();

    let mut child = match Command::new(&yt_cmd)
        .args([
            url,
            "--dump-json",
            "--flat-playlist",
            "--no-warnings",
            "--no-check-certificates",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let mut reader = tokio::io::BufReader::new(stdout).lines();
    let mut tracks = Vec::new();
    let mut playlist_title = None;

    while let Ok(Some(line)) = reader.next_line().await {
        if let Ok(item) = serde_json::from_str::<serde_json::Value>(&line) {
            if playlist_title.is_none() {
                if let Some(title) = item["playlist_title"].as_str() {
                    playlist_title = Some(title.to_string());
                } else if let Some(title) = item["playlist"].as_str() {
                    playlist_title = Some(title.to_string());
                }
            }

            if let Some(track) = parse_track_json(&item, &base_url) {
                tracks.push(track);
            }
        }
    }

    let _ = child.wait().await;

    Ok(Json(ExtractResponse {
        playlist_title,
        tracks,
    }))
}

async fn stream_audio(
    Query(params): Query<StreamParams>,
    _client_headers: HeaderMap,
) -> Result<Response, StatusCode> {
    let mut target = String::new();

    if let Some(u) = params.url {
        if !u.trim().is_empty() {
            target = u.trim().to_string();
        }
    }

    if target.is_empty() {
        if let Some(id) = params.id {
            let id = id.trim();
            if id.starts_with("http") {
                target = id.to_string();
            } else {
                target = format!("https://www.youtube.com/watch?v={}", id);
            }
        }
    }

    if target.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let yt_cmd = get_yt_dlp_cmd();

    let output = match Command::new(&yt_cmd)
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
    {
        Ok(out) => out,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    if !output.status.success() {
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

    let mut ffmpeg_child = match Command::new("ffmpeg")
        .args(&ffmpeg_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let stdout = match ffmpeg_child.stdout.take() {
        Some(s) => s,
        None => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let stream = ReaderStream::new(stdout);
    let body = Body::from_stream(stream);

    let mut res_headers = HeaderMap::new();
    res_headers.insert(header::CONTENT_TYPE, "audio/mpeg".parse().unwrap());
    res_headers.insert(header::CACHE_CONTROL, "no-cache, no-store".parse().unwrap());
    res_headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap());
    res_headers.insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, "*".parse().unwrap());

    Ok((StatusCode::OK, res_headers, body).into_response())
}

async fn get_library() -> Result<Json<serde_json::Value>, StatusCode> {
    if Path::new("library.json").exists() {
        if let Ok(content) = std::fs::read_to_string("library.json") {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                return Ok(Json(data));
            }
        }
    }
    Ok(Json(serde_json::json!({
        "tracks": [],
        "playlists": [],
        "radio_stations": []
    })))
}

async fn save_library(Json(data): Json<serde_json::Value>) -> Result<StatusCode, StatusCode> {
    if let Ok(json_str) = serde_json::to_string_pretty(&data) {
        if std::fs::write("library.json", json_str).is_ok() {
            return Ok(StatusCode::OK);
        }
    }
    Err(StatusCode::INTERNAL_SERVER_ERROR)
}

#[tokio::main]
async fn main() {
    let cors = CorsLayer::permissive();

    let app = Router::new()
        .route("/", get(|| async { "SIGNAL // Audio Backend is running" }))
        .route("/api/health", get(health_check))
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
