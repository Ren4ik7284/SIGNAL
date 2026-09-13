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

fn get_cookies_path() -> Option<String> {
    if let Ok(env_path) = std::env::var("YT_COOKIES_PATH") {
        if Path::new(&env_path).exists() {
            return Some(env_path);
        }
    }
    if Path::new("cookies.txt").exists() {
        return Some("cookies.txt".to_string());
    }
    if Path::new("backend/cookies.txt").exists() {
        return Some("backend/cookies.txt".to_string());
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let candidates = [
        format!("{}/music-player/backend/cookies.txt", home),
        format!("{}/.config/yt-dlp/cookies.txt", home),
    ];
    for c in candidates {
        if Path::new(&c).exists() {
            return Some(c);
        }
    }
    None
}

fn has_chromium_profile() -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    Path::new(&format!("{}/.config/chromium", home)).exists()
        || Path::new(&format!("{}/.config/google-chrome", home)).exists()
}

fn apply_yt_dlp_common_args(cmd: &mut Command) {
    cmd.stdin(Stdio::null());
    cmd.stderr(Stdio::null());
    cmd.args([
        "--no-warnings",
        "--no-check-certificates",
        "--remote-components",
        "ejs:github",
    ]);
    if let Some(cookies) = get_cookies_path() {
        cmd.arg("--cookies").arg(cookies);
    } else if has_chromium_profile() {
        cmd.args(["--cookies-from-browser", "chromium"]);
    }
}

async fn ensure_cookies_on_start() {
    if get_cookies_path().is_some() {
        println!("[SIGNAL] YouTube cookies found.");
        return;
    }
    if !has_chromium_profile() {
        println!("[SIGNAL] Running in container or without Chromium profile, skipping cookie auto-export.");
        return;
    }
    let yt_cmd = get_yt_dlp_cmd();
    println!("[SIGNAL] Cookies not found. Attempting auto-export from chromium...");
    let res = Command::new(&yt_cmd)
        .args([
            "--cookies",
            "cookies.txt",
            "--cookies-from-browser",
            "chromium",
            "--skip-download",
            "https://www.youtube.com",
        ])
        .output()
        .await;
    match res {
        Ok(out) if out.status.success() => {
            println!("[SIGNAL] Successfully exported YouTube cookies from chromium!");
        }
        _ => {
            println!("[SIGNAL] Note: unable to auto-export cookies from chromium.");
        }
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

    let is_direct_url = query.starts_with("http://") || query.starts_with("https://");
    let search_arg = if is_direct_url {
        query.to_string()
    } else {
        format!("ytsearch15:{}", query)
    };

    println!("[search] Performing search for: {}", query);
    let mut cmd = Command::new(&yt_cmd);
    apply_yt_dlp_common_args(&mut cmd);
    cmd.args([
        &search_arg,
        "--dump-json",
        "--flat-playlist",
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::null());

    let mut tracks = Vec::new();

    if let Ok(mut child) = cmd.spawn() {
        if let Some(stdout) = child.stdout.take() {
            let mut reader = tokio::io::BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if let Ok(item) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(track) = parse_track_json(&item, &base_url) {
                        tracks.push(track);
                    }
                }
            }
        }
        let _ = child.wait().await;
    }

    // Secondary fallback to SoundCloud if YouTube search returned 0 items
    if tracks.is_empty() && !is_direct_url {
        println!("[search] YouTube returned 0 results, trying SoundCloud fallback...");
        let sc_arg = format!("scsearch10:{}", query);
        let mut sc_cmd = Command::new(&yt_cmd);
        apply_yt_dlp_common_args(&mut sc_cmd);
        sc_cmd.args([
            &sc_arg,
            "--dump-json",
            "--flat-playlist",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

        if let Ok(mut sc_child) = sc_cmd.spawn() {
            if let Some(sc_stdout) = sc_child.stdout.take() {
                let mut sc_reader = tokio::io::BufReader::new(sc_stdout).lines();
                while let Ok(Some(line)) = sc_reader.next_line().await {
                    if let Ok(item) = serde_json::from_str::<serde_json::Value>(&line) {
                        if let Some(track) = parse_track_json(&item, &base_url) {
                            tracks.push(track);
                        }
                    }
                }
            }
            let _ = sc_child.wait().await;
        }
    }

    println!("[search] Found {} tracks for: {}", tracks.len(), query);
    Ok(Json(tracks))
}

async fn extract_info(Query(params): Query<ExtractParams>) -> Result<Json<ExtractResponse>, StatusCode> {
    let url = params.url.trim();
    if url.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let yt_cmd = get_yt_dlp_cmd();
    let base_url = get_base_url();

    let mut cmd = Command::new(&yt_cmd);
    apply_yt_dlp_common_args(&mut cmd);
    cmd.args([
        url,
        "--dump-json",
        "--flat-playlist",
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::null());

    let mut child = match cmd.spawn() {
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
    let mut direct_url = String::new();

    if target.ends_with(".mp3")
        || target.ends_with(".aac")
        || target.ends_with(".aacp")
        || target.ends_with(".m3u8")
        || target.contains("/stream/")
        || target.contains(":80")
    {
        direct_url = target.clone();
    } else {
        println!("[stream] Resolving audio stream for: {}", target);
        // 1. Direct extraction with yt-dlp
        let mut cmd = Command::new(&yt_cmd);
        cmd.args(["-g", "-f", "bestaudio/ba/b"]);
        apply_yt_dlp_common_args(&mut cmd);
        cmd.arg(&target);

        if let Ok(Ok(out)) = tokio::time::timeout(std::time::Duration::from_secs(12), cmd.output()).await {
            if out.status.success() {
                let u = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !u.is_empty() {
                    direct_url = u;
                }
            }
        }

        // 2. Fallback if direct extraction failed (e.g. SoundCloud DRM, or geo-locked track)
        if direct_url.is_empty() {
            println!("[stream] Direct extraction failed. Getting metadata for fallback search...");
            let mut info_cmd = Command::new(&yt_cmd);
            apply_yt_dlp_common_args(&mut info_cmd);
            info_cmd.args(["--dump-json", "--flat-playlist", "--ignore-no-formats-error", &target]);

            let mut resolved_title = String::new();
            let mut resolved_uploader = String::new();

            if let Ok(Ok(iout)) = tokio::time::timeout(std::time::Duration::from_secs(8), info_cmd.output()).await {
                if let Ok(item) = serde_json::from_slice::<serde_json::Value>(&iout.stdout) {
                    if let Some(t) = item["title"].as_str() {
                        resolved_title = t.to_string();
                    }
                    if let Some(u) = item["uploader"]
                        .as_str()
                        .or_else(|| item["artist"].as_str())
                        .or_else(|| item["channel"].as_str())
                    {
                        resolved_uploader = u.to_string();
                    }
                }
            }

            if resolved_title.is_empty() && target.contains("soundcloud.com/") {
                if let Some(path) = target.split("soundcloud.com/").nth(1) {
                    let parts: Vec<&str> = path.split('?').next().unwrap_or("").split('/').filter(|s| !s.is_empty()).collect();
                    if parts.len() >= 2 {
                        resolved_uploader = parts[0].replace('-', " ");
                        resolved_title = parts[1].replace('-', " ");
                    }
                }
            }

            if !resolved_title.is_empty() {
                let search_query = format!("ytsearch1:{} {}", resolved_title, resolved_uploader);
                println!("[stream] Trying YouTube search fallback: {}", search_query);
                let mut yt_fallback = Command::new(&yt_cmd);
                yt_fallback.args(["-g", "-f", "bestaudio/ba/b"]);
                apply_yt_dlp_common_args(&mut yt_fallback);
                yt_fallback.arg(&search_query);

                if let Ok(Ok(sc)) = tokio::time::timeout(std::time::Duration::from_secs(10), yt_fallback.output()).await {
                    if sc.status.success() {
                        let u = String::from_utf8_lossy(&sc.stdout).trim().to_string();
                        if !u.is_empty() {
                            direct_url = u;
                        }
                    }
                }

                if direct_url.is_empty() {
                    let sc_query = format!("scsearch1:{} {}", resolved_title, resolved_uploader);
                    println!("[stream] Trying SoundCloud search fallback: {}", sc_query);
                    let mut sc_fallback = Command::new(&yt_cmd);
                    sc_fallback.args(["-g", "-f", "bestaudio/b"]);
                    apply_yt_dlp_common_args(&mut sc_fallback);
                    sc_fallback.arg(&sc_query);

                    if let Ok(Ok(sc)) = tokio::time::timeout(std::time::Duration::from_secs(8), sc_fallback.output()).await {
                        if sc.status.success() {
                            let u = String::from_utf8_lossy(&sc.stdout).trim().to_string();
                            if !u.is_empty() {
                                direct_url = u;
                            }
                        }
                    }
                }
            }
        }
    }

    if direct_url.is_empty() {
        eprintln!("[stream] Failed to resolve playable URL for: {}", target);
        return Err(StatusCode::NOT_FOUND);
    }

    println!("[stream] Direct audio URL resolved successfully, starting ffmpeg transcode...");
    let mut ffmpeg_args = vec![
        "-user_agent".to_string(),
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36".to_string(),
        "-referer".to_string(),
        "https://www.youtube.com/".to_string(),
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

    println!("[stream] Running ffmpeg with url length: {}", direct_url.len());
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
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[stream] Failed to spawn ffmpeg: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let stdout = match ffmpeg_child.stdout.take() {
        Some(s) => s,
        None => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    tokio::spawn(async move {
        let status = ffmpeg_child.wait().await;
        println!("[stream] FFmpeg exited with: {:?}", status);
    });

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
    ensure_cookies_on_start().await;

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
