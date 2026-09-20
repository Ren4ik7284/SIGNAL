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

use crate::config::{apply_yt_dlp_common_args, get_yt_dlp_cmd, is_cloud_env, CLOUD_FALLBACK_URL};
use crate::models::StreamParams;

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
        if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(15), sc_cmd.output()).await {
            if out.status.success() {
                let u = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !u.is_empty() {
                    direct_url = u;
                }
            }
        }
    } else {
        println!("[stream] Resolving audio stream for: {}", target);
        let mut cmd = Command::new(&yt_cmd);
        cmd.args(["--no-playlist", "-g", "-f", "bestaudio/ba/b"]);
        apply_yt_dlp_common_args(&mut cmd);
        cmd.arg(&target);

        if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(12), cmd.output()).await {
            if out.status.success() {
                let u = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !u.is_empty() {
                    direct_url = u;
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
                let clean_uploader = resolved_uploader.replace(" - Topic", "").trim().to_string();
                let sc_query = if !clean_uploader.is_empty() && !resolved_title.to_lowercase().contains(&clean_uploader.to_lowercase()) {
                    format!("scsearch1:{} {}", resolved_title, clean_uploader)
                } else {
                    format!("scsearch1:{}", resolved_title)
                };

                println!("[stream] Trying SoundCloud fallback: {}", sc_query);
                let mut sc_fallback = Command::new(&yt_cmd);
                sc_fallback.args(["-g", "-f", "bestaudio/b"]);
                apply_yt_dlp_common_args(&mut sc_fallback);
                sc_fallback.arg(&sc_query);

                if let Ok(Ok(sc)) = tokio::time::timeout(Duration::from_secs(15), sc_fallback.output()).await {
                    if sc.status.success() {
                        let u = String::from_utf8_lossy(&sc.stdout).trim().to_string();
                        if !u.is_empty() {
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
                let inv_url = format!("https://inv.tux.pizza/api/v1/videos/{}", v);
                if let Ok(client) = reqwest::Client::builder().timeout(Duration::from_secs(3)).build() {
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
