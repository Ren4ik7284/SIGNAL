use axum::{
    body::Body,
    extract::{ConnectInfo, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use std::net::SocketAddr;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio_util::io::ReaderStream;

use crate::config::{apply_yt_dlp_common_args, apply_yt_dlp_common_args_no_cookies, get_yt_dlp_cmd, is_cloud_env, CLOUD_FALLBACK_URL};
use crate::models::StreamParams;
use crate::security::{check_rate_limit, check_url_ssrf, get_client_ip};
use crate::AppState;

fn extract_stream_url_from_output(out: &std::process::Output) -> Option<String> {
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines() {
        let trimmed = line.trim();
        if (trimmed.starts_with("http://") || trimmed.starts_with("https://"))
            && (trimmed.contains("googlevideo.com")
                || trimmed.contains("soundcloud")
                || trimmed.contains("sndcdn")
                || trimmed.contains(".m3u8")
                || trimmed.contains(".mp3")
                || trimmed.contains(".aac"))
        {
            return Some(trimmed.to_string());
        }
    }
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn clean_music_title(title: &str) -> String {
    let mut s = title.to_string();
    while let Some(open) = s.find('[') {
        if let Some(close) = s[open..].find(']') {
            s.replace_range(open..=open + close, " ");
        } else {
            break;
        }
    }
    while let Some(open) = s.find('(') {
        if let Some(close) = s[open..].find(')') {
            s.replace_range(open..=open + close, " ");
        } else {
            break;
        }
    }
    while let Some(open) = s.find('{') {
        if let Some(close) = s[open..].find('}') {
            s.replace_range(open..=open + close, " ");
        } else {
            break;
        }
    }
    if let Some(pipe) = s.find('|') {
        s.truncate(pipe);
    }
    let mut cleaned = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_alphanumeric() || c == ' ' || c == '-' {
            cleaned.push(c);
        } else {
            cleaned.push(' ');
        }
    }

    let noise_words = [
        "official music video", "official video", "official audio", "official",
        "lyric video", "lyrics", "visualizer", "audio", "clip officiel",
        "remastered", "4k", "hd", "hq", "live in", "full album", "сборник песен",
        "премьера клипа", "премьера песни", "клип", "новинки", "новинка", "хиты",
        "music video", "original mix", "extended mix", "topic",
    ];

    let mut lower = cleaned.to_lowercase();
    for word in noise_words {
        while let Some(idx) = lower.find(word) {
            cleaned.replace_range(idx..idx + word.len(), " ");
            lower.replace_range(idx..idx + word.len(), " ");
        }
    }

    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub async fn stream_audio(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(params): Query<StreamParams>,
) -> Result<Response, StatusCode> {
    let client_ip = get_client_ip(&headers, Some(addr));
    check_rate_limit(
        &state.endpoint_rate_limits,
        &format!("stream:{}", client_ip),
        40,
        60,
    )?;

    let _permit = match tokio::time::timeout(
        Duration::from_millis(2000),
        state.heavy_process_semaphore.acquire(),
    )
    .await
    {
        Ok(Ok(permit)) => permit,
        _ => return Err(StatusCode::TOO_MANY_REQUESTS),
    };

    let mut target = String::new();

    if let Some(u) = params.url.clone() {
        if !u.trim().is_empty() {
            target = u.trim().to_string();
        }
    }

    if target.is_empty() {
        if let Some(id) = params.id.clone() {
            let id = id.trim().to_string();
            if id.starts_with("http") {
                target = id;
            } else {
                target = format!("https://www.youtube.com/watch?v={}", id);
            }
        }
    }

    if target.is_empty() {
        if let Some(title) = params.title.as_deref() {
            let t = title.trim();
            if !t.is_empty() {
                let artist = params.artist.as_deref().unwrap_or("").trim();
                let clean = clean_music_title(t);
                if !artist.is_empty() {
                    target = format!("scsearch2:{} {}", clean, artist);
                } else {
                    target = format!("scsearch2:{}", clean);
                }
            }
        }
    }

    if target.is_empty() || target.starts_with('-') {
        return Err(StatusCode::BAD_REQUEST);
    }

    if target.starts_with("http://") || target.starts_with("https://") {
        let is_trusted_music_domain = target.contains("youtube.com")
            || target.contains("youtu.be")
            || target.contains("soundcloud.com")
            || target.contains("sndcdn.com");
        if !is_trusted_music_domain {
            check_url_ssrf(&target).await?;
        }
    }

    let yt_cmd = get_yt_dlp_cmd();
    let mut direct_url = String::new();

    let is_direct_candidate = target.starts_with("http://") || target.starts_with("https://");
    if is_direct_candidate
        && (target.ends_with(".mp3")
            || target.ends_with(".aac")
            || target.ends_with(".aacp")
            || target.ends_with(".m3u8")
            || target.contains("/stream/")
            || target.contains(":80"))
    {
        check_url_ssrf(&target).await?;
        direct_url = target.clone();
    } else if target.contains("soundcloud.com") || target.starts_with("scsearch") {
        let mut sc_cmd = Command::new(&yt_cmd);
        apply_yt_dlp_common_args_no_cookies(&mut sc_cmd);
        sc_cmd.args(["--no-playlist", "--ignore-errors", "-g", "-f", "bestaudio/b", "--", &target]);
        if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(6), sc_cmd.output()).await {
            if let Some(u) = extract_stream_url_from_output(&out) {
                direct_url = u;
            }
        }

        if direct_url.is_empty() {
            let mut sc_cmd2 = Command::new(&yt_cmd);
            apply_yt_dlp_common_args(&mut sc_cmd2);
            sc_cmd2.args(["--no-playlist", "--ignore-errors", "-g", "-f", "bestaudio/b", "--", &target]);
            if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(4), sc_cmd2.output()).await {
                if let Some(u) = extract_stream_url_from_output(&out) {
                    direct_url = u;
                }
            }
        }

        if direct_url.is_empty() {
            let mut title = params.title.as_deref().unwrap_or("").trim().to_string();
            let mut artist = params.artist.as_deref().unwrap_or("").trim().to_string();

            // If title is missing, extract from SoundCloud URL slug
            if title.is_empty() && target.contains("soundcloud.com/") {
                if let Some(path) = target.split("soundcloud.com/").nth(1) {
                    let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
                    if parts.len() >= 2 {
                        artist = parts[0].replace('-', " ");
                        title = parts[1].replace('-', " ");
                    } else if parts.len() == 1 {
                        title = parts[0].replace('-', " ");
                    }
                }
            }

            let clean = clean_music_title(&title);
            let clean_art = clean_music_title(&artist);

            if !clean.is_empty() {
                let query = if !clean_art.is_empty() {
                    format!("scsearch5:{} {}", clean, clean_art)
                } else {
                    format!("scsearch5:{}", clean)
                };
                let mut alt_cmd = Command::new(&yt_cmd);
                apply_yt_dlp_common_args_no_cookies(&mut alt_cmd);
                alt_cmd.args(["--no-playlist", "--ignore-errors", "-g", "-f", "bestaudio/b", "--", &query]);
                if let Ok(Ok(alt_out)) = tokio::time::timeout(Duration::from_secs(6), alt_cmd.output()).await {
                    if let Some(u) = extract_stream_url_from_output(&alt_out) {
                        direct_url = u;
                    }
                }
            }

            if direct_url.is_empty() && !clean.is_empty() && !clean_art.is_empty() {
                let query = format!("scsearch5:{}", clean);
                let mut alt_cmd = Command::new(&yt_cmd);
                apply_yt_dlp_common_args_no_cookies(&mut alt_cmd);
                alt_cmd.args(["--no-playlist", "--ignore-errors", "-g", "-f", "bestaudio/b", "--", &query]);
                if let Ok(Ok(alt_out)) = tokio::time::timeout(Duration::from_secs(5), alt_cmd.output()).await {
                    if let Some(u) = extract_stream_url_from_output(&alt_out) {
                        direct_url = u;
                    }
                }
            }
        }
    } else {
        let mut cmd_fast = Command::new(&yt_cmd);
        apply_yt_dlp_common_args(&mut cmd_fast);
        cmd_fast.args([
            "--no-playlist",
            "--ignore-errors",
            "-g",
            "-f", "bestaudio/ba/b",
            "--extractor-args", "youtube:player_client=ios,web,mweb",
            "--",
            &target,
        ]);
        if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_millis(3000), cmd_fast.output()).await {
            if let Some(u) = extract_stream_url_from_output(&out) {
                direct_url = u;
            }
        }

        let mut resolved_title = params.title.as_deref().unwrap_or("").trim().to_string();
        let mut resolved_uploader = params.artist.as_deref().unwrap_or("").trim().to_string();

        if direct_url.is_empty() && resolved_title.is_empty() {
            let oembed_url = format!(
                "https://www.youtube.com/oembed?url={}&format=json",
                urlencoding::encode(&target)
            );
            if let Ok(client) = reqwest::Client::builder().timeout(Duration::from_millis(1500)).build() {
                if let Ok(resp) = client.get(&oembed_url).send().await {
                    if resp.status().is_success() {
                        if let Ok(bytes) = resp.bytes().await {
                            if let Ok(data) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                                if let Some(t) = data["title"].as_str() {
                                    resolved_title = t.trim().to_string();
                                }
                                if let Some(a) = data["author_name"].as_str() {
                                    resolved_uploader = a.trim().to_string();
                                }
                            }
                        }
                    }
                }
            }
        }

        // Fast & reliable SoundCloud fallback search for YouTube tracks
        if direct_url.is_empty() && !resolved_title.is_empty() {
            let clean_title = clean_music_title(&resolved_title);
            let clean_uploader = clean_music_title(&resolved_uploader);
            let sc_query = if !clean_uploader.is_empty() && !clean_title.to_lowercase().contains(&clean_uploader.to_lowercase()) {
                format!("scsearch5:{} {}", clean_title, clean_uploader)
            } else {
                format!("scsearch5:{}", clean_title)
            };

            let mut sc_fallback = Command::new(&yt_cmd);
            apply_yt_dlp_common_args_no_cookies(&mut sc_fallback);
            sc_fallback.args(["--no-playlist", "--ignore-errors", "-g", "-f", "bestaudio/b", "--", &sc_query]);

            if let Ok(Ok(sc)) = tokio::time::timeout(Duration::from_secs(6), sc_fallback.output()).await {
                if let Some(u) = extract_stream_url_from_output(&sc) {
                    direct_url = u;
                }
            }

            if direct_url.is_empty() && !clean_uploader.is_empty() {
                let sc_title_query = format!("scsearch5:{}", clean_title);
                let mut sc_title_fb = Command::new(&yt_cmd);
                apply_yt_dlp_common_args_no_cookies(&mut sc_title_fb);
                sc_title_fb.args(["--no-playlist", "--ignore-errors", "-g", "-f", "bestaudio/b", "--", &sc_title_query]);

                if let Ok(Ok(sc)) = tokio::time::timeout(Duration::from_secs(5), sc_title_fb.output()).await {
                    if let Some(u) = extract_stream_url_from_output(&sc) {
                        direct_url = u;
                    }
                }
            }
        }

        if direct_url.is_empty() {
            let vid = if let Some(idx) = target.find("v=") {
                let rest = &target[idx + 2..];
                let end = rest.find(['&', '?', '#', '/']).unwrap_or(rest.len());
                Some(rest[..end].to_string())
            } else if let Some(idx) = target.find("youtu.be/") {
                let rest = &target[idx + 9..];
                let end = rest.find(['&', '?', '#', '/']).unwrap_or(rest.len());
                Some(rest[..end].to_string())
            } else {
                None
            };

            if let Some(v) = vid {
                let instances = [
                    "https://inv.nadeko.net",
                    "https://invidious.nerdvpn.de",
                    "https://vid.priv.au",
                    "https://invidious.private.coffee",
                    "https://yt.drgnz.club"
                ];
                if let Ok(client) = reqwest::Client::builder().timeout(Duration::from_secs(2)).build() {
                    for inst in instances {
                        let inv_url = format!("{}/api/v1/videos/{}", inst, v);
                        if let Ok(resp) = client.get(&inv_url).send().await {
                            if resp.status().is_success() {
                                if let Ok(bytes) = resp.bytes().await {
                                    if let Ok(data) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                                        if let Some(formats) = data["adaptiveFormats"].as_array() {
                                            for f in formats {
                                                if let Some(t) = f["type"].as_str() {
                                                    if t.starts_with("audio/") {
                                                        if let Some(u) = f["url"].as_str() {
                                                            if !u.is_empty() {
                                                                direct_url = u.to_string();
                                                                break;
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if !direct_url.is_empty() {
                            break;
                        }
                    }
                }
            }
        }

        if direct_url.is_empty() && !is_cloud_env() {
            let cloud_stream_url = format!(
                "{}/api/stream?url={}{}{}{}",
                CLOUD_FALLBACK_URL,
                urlencoding::encode(&target),
                params.ss.map(|s| format!("&ss={}", s)).unwrap_or_default(),
                params.title.as_deref().map(|t| format!("&title={}", urlencoding::encode(t))).unwrap_or_default(),
                params.artist.as_deref().map(|a| format!("&artist={}", urlencoding::encode(a))).unwrap_or_default()
            );

            if let Ok(client) = reqwest::Client::builder().timeout(Duration::from_secs(6)).build() {
                if let Ok(resp) = client.get(&cloud_stream_url).send().await {
                    if resp.status().is_success() {
                        let stream = resp.bytes_stream();
                        let body = Body::from_stream(stream);
                        let mut res_headers = HeaderMap::new();
                        res_headers.insert(header::CONTENT_TYPE, "audio/mpeg".parse().unwrap());
                        res_headers.insert(header::CACHE_CONTROL, "no-cache, no-store, must-revalidate".parse().unwrap());
                        res_headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap());
                        res_headers.insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, "*".parse().unwrap());
                        return Ok((StatusCode::OK, res_headers, body).into_response());
                    }
                }
            }
        }
    }

    if direct_url.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    let referer = if direct_url.contains("soundcloud") || direct_url.contains("sndcdn") {
        "https://soundcloud.com/"
    } else {
        "https://www.youtube.com/"
    };

    let mut ffmpeg_args = vec![
        "-protocol_whitelist".to_string(),
        "http,https,tcp,tls,crypto".to_string(),
        "-user_agent".to_string(),
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36".to_string(),
        "-referer".to_string(),
        referer.to_string(),
        "-reconnect".to_string(),
        "1".to_string(),
        "-reconnect_streamed".to_string(),
        "1".to_string(),
        "-reconnect_delay_max".to_string(),
        "5".to_string(),
        "-fflags".to_string(),
        "+nobuffer".to_string(),
        "-probesize".to_string(),
        "65536".to_string(),
        "-analyzeduration".to_string(),
        "200000".to_string(),
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

    let mut ffmpeg_cmd = Command::new("ffmpeg");
    ffmpeg_cmd.kill_on_drop(true);
    let mut ffmpeg_child = match ffmpeg_cmd
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

    tokio::spawn(async move {
        let _ = ffmpeg_child.wait().await;
    });

    let stream = ReaderStream::new(stdout);
    let body = Body::from_stream(stream);

    let mut res_headers = HeaderMap::new();
    res_headers.insert(header::CONTENT_TYPE, "audio/mpeg".parse().unwrap());
    res_headers.insert(header::CACHE_CONTROL, "no-cache, no-store, must-revalidate".parse().unwrap());
    res_headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap());
    res_headers.insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, "*".parse().unwrap());

    Ok((StatusCode::OK, res_headers, body).into_response())
}
