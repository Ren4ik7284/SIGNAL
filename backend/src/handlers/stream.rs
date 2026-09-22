use axum::{
    body::Body,
    extract::Query,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio_util::io::ReaderStream;

use crate::config::{apply_yt_dlp_common_args, apply_yt_dlp_common_args_no_cookies, get_yt_dlp_cmd, is_cloud_env, CLOUD_FALLBACK_URL};
use crate::models::StreamParams;

fn extract_stream_url_from_output(out: &std::process::Output) -> Option<String> {
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Prefer typical streaming domain URLs
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
    // Any valid http/https url
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
    // Remove brackets like [...]
    while let Some(open) = s.find('[') {
        if let Some(close) = s[open..].find(']') {
            s.replace_range(open..=open + close, " ");
        } else {
            break;
        }
    }
    // Remove common parenthetical noise like (Official Video), (Lyrics), (Audio), etc.
    let noise_patterns = [
        "official music video",
        "official video",
        "official audio",
        "lyric video",
        "lyrics",
        "visualizer",
        "audio",
        "clip officiel",
        "remastered",
        "4k",
        "hd",
        "hq",
        "live in",
        "full album",
        "official",
    ];
    let lower = s.to_lowercase();
    for pat in noise_patterns {
        if let Some(pos) = lower.find(pat) {
            let start = s[..pos].rfind('(');
            let end = s[pos..].find(')');
            if let (Some(open), Some(close)) = (start, end) {
                if pos + close < s.len() {
                    s.replace_range(open..=pos + close, " ");
                }
            }
        }
    }
    // Remove trailing pipes like "| ..."
    if let Some(pipe) = s.find('|') {
        s.truncate(pipe);
    }
    s = s.replace(" - Topic", "");
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub async fn stream_audio(
    Query(params): Query<StreamParams>,
    _client_headers: HeaderMap,
) -> Result<Response, StatusCode> {
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

    // Support streaming by title and artist directly
    if target.is_empty() {
        if let Some(title) = params.title.as_deref() {
            let t = title.trim();
            if !t.is_empty() {
                let artist = params.artist.as_deref().unwrap_or("").trim();
                let clean = clean_music_title(t);
                if !artist.is_empty() {
                    target = format!("scsearch5:{} {}", clean, artist);
                } else {
                    target = format!("scsearch5:{}", clean);
                }
            }
        }
    }

    if target.is_empty() || target.starts_with('-') {
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
    } else if target.contains("soundcloud.com") || target.starts_with("scsearch") {
        println!("[stream] Resolving SoundCloud stream for: {}", target);

        // Attempt 1: with cookies
        let mut sc_cmd = Command::new(&yt_cmd);
        apply_yt_dlp_common_args(&mut sc_cmd);
        sc_cmd.args(["-g", "-f", "bestaudio/b", "--", &target]);
        if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(15), sc_cmd.output()).await {
            if let Some(u) = extract_stream_url_from_output(&out) {
                direct_url = u;
            }
        }

        // Attempt 2: without cookies (SoundCloud rejects stale cookies)
        if direct_url.is_empty() {
            println!("[stream] SoundCloud with cookies failed, retrying without cookies...");
            let mut sc_cmd2 = Command::new(&yt_cmd);
            apply_yt_dlp_common_args_no_cookies(&mut sc_cmd2);
            sc_cmd2.args(["-g", "-f", "bestaudio/b", "--", &target]);
            if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(15), sc_cmd2.output()).await {
                if let Some(u) = extract_stream_url_from_output(&out) {
                    direct_url = u;
                }
            }
        }

        // If direct soundcloud URL failed (e.g. DRM protection on official tracks), fallback using title/artist!
        if direct_url.is_empty() {
            let title = params.title.as_deref().unwrap_or("").trim();
            let artist = params.artist.as_deref().unwrap_or("").trim();
            if !title.is_empty() {
                let clean = clean_music_title(title);
                println!("[stream] SoundCloud direct URL failed, searching alternative: {} {}", clean, artist);
                let query = if !artist.is_empty() {
                    format!("scsearch5:{} {}", clean, artist)
                } else {
                    format!("scsearch5:{}", clean)
                };
                let mut alt_cmd = Command::new(&yt_cmd);
                apply_yt_dlp_common_args(&mut alt_cmd);
                alt_cmd.args(["-g", "-f", "bestaudio/b", "--", &query]);
                if let Ok(Ok(alt_out)) = tokio::time::timeout(Duration::from_secs(15), alt_cmd.output()).await {
                    if let Some(u) = extract_stream_url_from_output(&alt_out) {
                        direct_url = u;
                    }
                }

                // If still empty, search YouTube for this track
                if direct_url.is_empty() {
                    let yt_query = format!("ytsearch3:{} {}", clean, artist);
                    println!("[stream] SoundCloud alternative failed, searching YouTube: {}", yt_query);
                    let mut yt_alt = Command::new(&yt_cmd);
                    apply_yt_dlp_common_args(&mut yt_alt);
                    yt_alt.args([
                        "-g",
                        "-f", "bestaudio/ba/b",
                        "--extractor-args", "youtube:player_client=ios,web",
                        "--",
                        &yt_query,
                    ]);
                    if let Ok(Ok(yt_out)) = tokio::time::timeout(Duration::from_secs(15), yt_alt.output()).await {
                        if let Some(u) = extract_stream_url_from_output(&yt_out) {
                            direct_url = u;
                        }
                    }
                }
            }
        }
    } else {
        println!("[stream] Resolving audio stream for: {}", target);

        // 1. Android client without cookies (bypasses YouTube 429 datacenter IP blocks completely)
        println!("[stream] Trying YouTube player_client=android (datacenter bypass) for: {}", target);
        let mut cmd_android = Command::new(&yt_cmd);
        apply_yt_dlp_common_args_no_cookies(&mut cmd_android);
        cmd_android.args([
            "--no-playlist",
            "-g",
            "-f", "bestaudio/b",
            "--extractor-args", "youtube:player_client=android",
            "--",
            &target,
        ]);
        if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(15), cmd_android.output()).await {
            if let Some(u) = extract_stream_url_from_output(&out) {
                direct_url = u;
            }
        }

        // 2. Standard yt-dlp with cookies
        if direct_url.is_empty() {
            println!("[stream] Trying standard yt-dlp resolution with cookies for: {}", target);
            let mut cmd_std = Command::new(&yt_cmd);
            apply_yt_dlp_common_args(&mut cmd_std);
            cmd_std.args([
                "--no-playlist",
                "-g",
                "-f", "bestaudio/ba/b",
                "--",
                &target,
            ]);
            if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(12), cmd_std.output()).await {
                if let Some(u) = extract_stream_url_from_output(&out) {
                    direct_url = u;
                }
            }
        }

        // 3. Web and mweb clients
        if direct_url.is_empty() {
            let yt_clients = ["web", "mweb"];
            for client in &yt_clients {
                if !direct_url.is_empty() {
                    break;
                }
                println!("[stream] Trying YouTube player_client={} for: {}", client, target);
                let mut cmd = Command::new(&yt_cmd);
                apply_yt_dlp_common_args(&mut cmd);
                cmd.args([
                    "--no-playlist",
                    "-g",
                    "-f", "bestaudio/ba/b",
                    "--extractor-args", &format!("youtube:player_client={}", client),
                    "--",
                    &target,
                ]);

                if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(12), cmd.output()).await {
                    if let Some(u) = extract_stream_url_from_output(&out) {
                        direct_url = u;
                    }
                }
            }
        }

        if direct_url.is_empty() && !is_cloud_env() {
            println!("[stream] Local extraction failed/blocked. Proxying stream from cloud backend...");
            let cloud_stream_url = format!(
                "{}/api/stream?url={}{}{}{}",
                CLOUD_FALLBACK_URL,
                urlencoding::encode(&target),
                params.ss.map(|s| format!("&ss={}", s)).unwrap_or_default(),
                params.title.as_deref().map(|t| format!("&title={}", urlencoding::encode(t))).unwrap_or_default(),
                params.artist.as_deref().map(|a| format!("&artist={}", urlencoding::encode(a))).unwrap_or_default()
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

        if direct_url.is_empty() {
            let mut resolved_title = params.title.as_deref().unwrap_or("").trim().to_string();
            let mut resolved_uploader = params.artist.as_deref().unwrap_or("").trim().to_string();

            if resolved_title.is_empty() {
                let oembed_url = format!(
                    "https://www.youtube.com/oembed?url={}&format=json",
                    urlencoding::encode(&target)
                );
                if let Ok(client) = reqwest::Client::builder().timeout(Duration::from_secs(3)).build() {
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

            if resolved_title.is_empty() {
                let mut info_cmd = Command::new(&yt_cmd);
                apply_yt_dlp_common_args(&mut info_cmd);
                info_cmd.args(["--no-playlist", "--dump-json", "--flat-playlist", "--ignore-no-formats-error", &target]);

                if let Ok(Ok(iout)) = tokio::time::timeout(Duration::from_secs(4), info_cmd.output()).await {
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
            }

            if !resolved_title.is_empty() {
                let clean_title = clean_music_title(&resolved_title);
                let clean_uploader = resolved_uploader.replace(" - Topic", "").trim().to_string();
                let sc_query = if !clean_uploader.is_empty() && !clean_title.to_lowercase().contains(&clean_uploader.to_lowercase()) {
                    format!("scsearch5:{} {}", clean_title, clean_uploader)
                } else {
                    format!("scsearch5:{}", clean_title)
                };

                println!("[stream] Trying SoundCloud fallback: {}", sc_query);
                let mut sc_fallback = Command::new(&yt_cmd);
                apply_yt_dlp_common_args(&mut sc_fallback);
                sc_fallback.args(["-g", "-f", "bestaudio/b"]);
                sc_fallback.arg(&sc_query);

                if let Ok(Ok(sc)) = tokio::time::timeout(Duration::from_secs(15), sc_fallback.output()).await {
                    if let Some(u) = extract_stream_url_from_output(&sc) {
                        direct_url = u;
                    }
                }

                // If SoundCloud fallback yielded nothing, try YouTube search with clean title
                if direct_url.is_empty() {
                    let yt_fb_query = format!("ytsearch3:{} {}", clean_title, clean_uploader);
                    println!("[stream] Trying YouTube search fallback: {}", yt_fb_query);
                    let mut yt_fb = Command::new(&yt_cmd);
                    apply_yt_dlp_common_args(&mut yt_fb);
                    yt_fb.args([
                        "-g",
                        "-f", "bestaudio/ba/b",
                        "--extractor-args", "youtube:player_client=android,web",
                        &yt_fb_query,
                    ]);
                    if let Ok(Ok(yt_out)) = tokio::time::timeout(Duration::from_secs(15), yt_fb.output()).await {
                        if let Some(u) = extract_stream_url_from_output(&yt_out) {
                            direct_url = u;
                        }
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
                let instances = ["https://inv.nadeko.net", "https://invidious.nerdvpn.de"];
                if let Ok(client) = reqwest::Client::builder().timeout(Duration::from_secs(4)).build() {
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
    }

    if direct_url.is_empty() {
        eprintln!("[stream] Failed to resolve playable URL for: {}", target);
        return Err(StatusCode::NOT_FOUND);
    }

    println!("[stream] Direct audio URL resolved successfully, starting ffmpeg transcode...");
    let referer = if direct_url.contains("soundcloud") || direct_url.contains("sndcdn") {
        "https://soundcloud.com/"
    } else {
        "https://www.youtube.com/"
    };

    let mut ffmpeg_args = vec![
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
