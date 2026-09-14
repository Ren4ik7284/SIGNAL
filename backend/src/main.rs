use axum::{
    body::Body,
    extract::Query,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tokio_util::io::ReaderStream;
use tower_http::cors::CorsLayer;

const CLOUD_FALLBACK_URL: &str = "https://signal-audio-backend-production.up.railway.app";

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

#[derive(Debug, Deserialize)]
pub struct CoverParams {
    pub url: String,
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

fn is_cloud_env() -> bool {
    std::env::var("RAILWAY_PUBLIC_DOMAIN").is_ok()
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
        "--extractor-args",
        "youtube:player_client=ios,web,android",
    ]);
    if let Ok(proxy) = std::env::var("YOUTUBE_PROXY") {
        let p = proxy.trim();
        if !p.is_empty() {
            cmd.arg("--proxy").arg(p);
        }
    }
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
    let id = if let Some(val) = item["id"].as_str() {
        val.to_string()
    } else if let Some(num) = item["id"].as_i64() {
        num.to_string()
    } else {
        return None;
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

    let proxied_cover = cover_url.map(|u| {
        if u.contains("ytimg.com") {
            format!("{}/api/cover?url={}", base_url, urlencoding::encode(&u))
        } else {
            u
        }
    });

    let encoded_url = urlencoding::encode(&track_url);
    let audio_url = format!("{}/api/stream?url={}", base_url, encoded_url);

    Some(SearchTrack {
        id,
        title,
        artist,
        duration,
        audio_url,
        cover_url: proxied_cover,
    })
}

async fn health_check() -> &'static str {
    "SIGNAL // Rust Engine Online"
}

async fn proxy_cover(Query(params): Query<CoverParams>) -> Result<Response, StatusCode> {
    let target = params.url.trim();
    if target.is_empty() || (!target.starts_with("http://") && !target.starts_with("https://")) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(6))
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let resp = client
        .get(target)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_string();

    let bytes = resp.bytes().await.map_err(|_| StatusCode::BAD_GATEWAY)?;

    let mut res_headers = HeaderMap::new();
    res_headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
    res_headers.insert(
        header::CACHE_CONTROL,
        "public, max-age=604800, immutable".parse().unwrap(),
    );
    res_headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap());

    Ok((StatusCode::OK, res_headers, bytes).into_response())
}

async fn execute_yt_dlp_search(yt_cmd: &str, search_arg: &str, timeout_sec: u64, base_url: &str) -> Vec<SearchTrack> {
    let mut cmd = Command::new(yt_cmd);
    apply_yt_dlp_common_args(&mut cmd);
    cmd.args([
        search_arg,
        "--dump-json",
        "--flat-playlist",
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::null());

    let mut tracks = Vec::new();

    let spawn_res = cmd.spawn();
    if let Ok(mut child) = spawn_res {
        if let Some(stdout) = child.stdout.take() {
            let mut reader = tokio::io::BufReader::new(stdout).lines();
            let read_task = async {
                while let Ok(Some(line)) = reader.next_line().await {
                    if let Ok(item) = serde_json::from_str::<serde_json::Value>(&line) {
                        if let Some(track) = parse_track_json(&item, base_url) {
                            tracks.push(track);
                        }
                    }
                }
            };
            let _ = tokio::time::timeout(Duration::from_secs(timeout_sec), read_task).await;
        }
        let _ = child.kill().await;
    }

    tracks
}

async fn execute_cloud_search(query: &str, base_url: &str) -> Vec<SearchTrack> {
    let cloud_url = format!("{}/api/search?q={}", CLOUD_FALLBACK_URL, urlencoding::encode(query));
    let client = match reqwest::Client::builder().timeout(Duration::from_secs(5)).build() {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    if let Ok(resp) = client.get(&cloud_url).send().await {
        if resp.status().is_success() {
            if let Ok(bytes) = resp.bytes().await {
                if let Ok(mut list) = serde_json::from_slice::<Vec<SearchTrack>>(&bytes) {
                    for t in &mut list {
                        if t.audio_url.contains("/api/stream") {
                            let stream_idx = t.audio_url.find("/api/stream").unwrap();
                            t.audio_url = format!("{}{}", base_url, &t.audio_url[stream_idx..]);
                        }
                    }
                    return list;
                }
            }
        }
    }

    Vec::new()
}

async fn search_music(Query(params): Query<SearchParams>) -> Result<Json<Vec<SearchTrack>>, StatusCode> {
    let query = params.q.trim();
    if query.is_empty() {
        return Ok(Json(Vec::new()));
    }

    let yt_cmd = get_yt_dlp_cmd();
    let base_url = get_base_url();
    let is_direct_url = query.starts_with("http://") || query.starts_with("https://");

    println!("[search] Performing search for: {}", query);

    if is_direct_url {
        let mut tracks = execute_yt_dlp_search(&yt_cmd, query, 8, &base_url).await;
        if tracks.is_empty() && !is_cloud_env() {
            let cloud_tracks = execute_cloud_search(query, &base_url).await;
            if !cloud_tracks.is_empty() {
                tracks = cloud_tracks;
            }
        }
        return Ok(Json(tracks));
    }

    let yt_arg = format!("ytsearch10:{}", query);
    let sc_arg = format!("scsearch10:{}", query);

    let (yt_res, sc_res) = tokio::join!(
        execute_yt_dlp_search(&yt_cmd, &yt_arg, 4, &base_url),
        execute_yt_dlp_search(&yt_cmd, &sc_arg, 4, &base_url),
    );

    let mut combined = Vec::new();
    let mut seen_ids = HashSet::new();

    for t in yt_res {
        if seen_ids.insert(t.id.clone()) {
            combined.push(t);
        }
    }

    // If YouTube search was blocked / empty locally and we are not in cloud, try cloud search
    if combined.is_empty() && !is_cloud_env() {
        println!("[search] Local YouTube search empty/blocked, querying cloud engine...");
        let cloud_tracks = execute_cloud_search(query, &base_url).await;
        for t in cloud_tracks {
            if seen_ids.insert(t.id.clone()) {
                combined.push(t);
            }
        }
    }

    for t in sc_res {
        if seen_ids.insert(t.id.clone()) {
            combined.push(t);
        }
    }

    println!("[search] Found {} tracks for: {}", combined.len(), query);
    Ok(Json(combined))
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

    let read_task = async {
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
    };

    let _ = tokio::time::timeout(Duration::from_secs(10), read_task).await;
    let _ = child.kill().await;

    // Fallback to cloud extract if local extraction yielded 0 items
    if tracks.is_empty() && !is_cloud_env() {
        let cloud_url = format!("{}/api/extract?url={}", CLOUD_FALLBACK_URL, urlencoding::encode(url));
        if let Ok(client) = reqwest::Client::builder().timeout(Duration::from_secs(8)).build() {
            if let Ok(resp) = client.get(&cloud_url).send().await {
                if resp.status().is_success() {
                    if let Ok(bytes) = resp.bytes().await {
                        if let Ok(mut ext_resp) = serde_json::from_slice::<ExtractResponse>(&bytes) {
                            for t in &mut ext_resp.tracks {
                                if t.audio_url.contains("/api/stream") {
                                    let stream_idx = t.audio_url.find("/api/stream").unwrap();
                                    t.audio_url = format!("{}{}", base_url, &t.audio_url[stream_idx..]);
                                }
                            }
                            return Ok(Json(ext_resp));
                        }
                    }
                }
            }
        }
    }

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
    } else if target.contains("soundcloud.com") {
        println!("[stream] Resolving SoundCloud stream for: {}", target);
        let mut sc_cmd = Command::new(&yt_cmd);
        apply_yt_dlp_common_args(&mut sc_cmd);
        sc_cmd.args(["-g", "-f", "bestaudio/b", &target]);
        if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(6), sc_cmd.output()).await {
            if out.status.success() {
                let u = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !u.is_empty() {
                    direct_url = u;
                }
            }
        }
    } else {
        println!("[stream] Resolving audio stream for: {}", target);
        // 1. Direct extraction with yt-dlp
        let mut cmd = Command::new(&yt_cmd);
        cmd.args(["-g", "-f", "bestaudio/ba/b"]);
        apply_yt_dlp_common_args(&mut cmd);
        cmd.arg(&target);

        if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(5), cmd.output()).await {
            if out.status.success() {
                let u = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !u.is_empty() {
                    direct_url = u;
                }
            }
        }

        // 2. Fallback to Cloud Proxy Stream if local extraction failed (e.g. YouTube blocked in Russia or bot-check)
        if direct_url.is_empty() && !is_cloud_env() {
            println!("[stream] Local extraction failed/blocked. Proxying stream from cloud backend...");
            let cloud_stream_url = format!(
                "{}/api/stream?url={}{}",
                CLOUD_FALLBACK_URL,
                urlencoding::encode(&target),
                params.ss.map(|s| format!("&ss={}", s)).unwrap_or_default()
            );

            if let Ok(client) = reqwest::Client::builder().timeout(Duration::from_secs(12)).build() {
                if let Ok(resp) = client.get(&cloud_stream_url).send().await {
                    if resp.status().is_success() {
                        let stream = resp.bytes_stream();
                        let body = Body::from_stream(stream);
                        let mut res_headers = HeaderMap::new();
                        res_headers.insert(header::CONTENT_TYPE, "audio/mpeg".parse().unwrap());
                        res_headers.insert(header::ACCEPT_RANGES, "bytes".parse().unwrap());
                        res_headers.insert(header::CACHE_CONTROL, "no-cache, no-store".parse().unwrap());
                        res_headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap());
                        res_headers.insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, "*".parse().unwrap());
                        return Ok((StatusCode::OK, res_headers, body).into_response());
                    }
                }
            }
        }

        // 3. Fallback to SoundCloud search
        if direct_url.is_empty() {
            println!("[stream] Getting track metadata for SoundCloud fallback search...");
            let mut info_cmd = Command::new(&yt_cmd);
            apply_yt_dlp_common_args(&mut info_cmd);
            info_cmd.args(["--dump-json", "--flat-playlist", "--ignore-no-formats-error", &target]);

            let mut resolved_title = String::new();
            let mut resolved_uploader = String::new();

            if let Ok(Ok(iout)) = tokio::time::timeout(Duration::from_secs(5), info_cmd.output()).await {
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

            if !resolved_title.is_empty() {
                let sc_query = format!("scsearch1:{} {}", resolved_title, resolved_uploader);
                println!("[stream] Trying SoundCloud search fallback: {}", sc_query);
                let mut sc_fallback = Command::new(&yt_cmd);
                sc_fallback.args(["-g", "-f", "bestaudio/b"]);
                apply_yt_dlp_common_args(&mut sc_fallback);
                sc_fallback.arg(&sc_query);

                if let Ok(Ok(sc)) = tokio::time::timeout(Duration::from_secs(5), sc_fallback.output()).await {
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

    ffmpeg_args.extend([
        "-i".to_string(),
        direct_url,
        "-vn".to_string(),
        "-f".to_string(),
        "mp3".to_string(),
        "-b:a".to_string(),
        "192k".to_string(),
        "-flush_packets".to_string(),
        "1".to_string(),
        "pipe:1".to_string(),
    ]);

    let mut ffmpeg_child = match Command::new("ffmpeg")
        .args(&ffmpeg_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
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
        let _ = ffmpeg_child.wait().await;
    });

    let stream = ReaderStream::new(stdout);
    let body = Body::from_stream(stream);

    let mut res_headers = HeaderMap::new();
    res_headers.insert(header::CONTENT_TYPE, "audio/mpeg".parse().unwrap());
    res_headers.insert(header::ACCEPT_RANGES, "bytes".parse().unwrap());
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
