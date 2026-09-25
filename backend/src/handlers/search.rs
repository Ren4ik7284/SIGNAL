use axum::{
    extract::{ConnectInfo, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

use crate::config::{apply_yt_dlp_common_args, get_base_url, get_yt_dlp_cmd, is_cloud_env, CLOUD_FALLBACK_URL};
use crate::models::{ExtractParams, ExtractResponse, SearchParams, SearchTrack};
use crate::security::{check_rate_limit, check_url_ssrf, get_client_ip};
use crate::ytdlp::{execute_cloud_search, execute_yt_dlp_search, parse_track_json};
use crate::AppState;

pub async fn search_music(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(params): Query<SearchParams>,
) -> Result<Json<Vec<SearchTrack>>, StatusCode> {
    let client_ip = get_client_ip(&headers, Some(addr));
    check_rate_limit(
        &state.endpoint_rate_limits,
        &format!("search:{}", client_ip),
        30,
        60,
    )?;

    let query = params.q.trim();
    if query.is_empty() {
        return Ok(Json(Vec::new()));
    }

    let _permit = match tokio::time::timeout(
        Duration::from_millis(2000),
        state.heavy_process_semaphore.acquire(),
    )
    .await
    {
        Ok(Ok(permit)) => permit,
        _ => return Err(StatusCode::TOO_MANY_REQUESTS),
    };

    let yt_cmd = get_yt_dlp_cmd();
    let base_url = get_base_url();
    let is_direct_url = query.starts_with("http://") || query.starts_with("https://");

    if is_direct_url {
        let is_trusted = query.contains("youtube.com")
            || query.contains("youtu.be")
            || query.contains("soundcloud.com");
        if !is_trusted {
            check_url_ssrf(query).await?;
        }

        let mut tracks = execute_yt_dlp_search(&yt_cmd, query, 15, &base_url).await;
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

    let (audius_res, yt_res, sc_res) = tokio::join!(
        execute_audius_search(query, 6, &base_url),
        execute_yt_dlp_search(&yt_cmd, &yt_arg, 10, &base_url),
        execute_yt_dlp_search(&yt_cmd, &sc_arg, 10, &base_url),
    );

    let mut combined = Vec::new();
    let mut seen_ids = HashSet::new();

    let is_valid_duration = |duration: f64| -> bool {
        duration == 0.0 || (duration >= 30.0 && duration <= 600.0)
    };

    let sc_filtered: Vec<SearchTrack> = sc_res
        .into_iter()
        .filter(|t| is_valid_duration(t.duration))
        .collect();

    let yt_filtered: Vec<SearchTrack> = yt_res
        .into_iter()
        .filter(|t| is_valid_duration(t.duration))
        .collect();

    // First add Audius tracks (direct instant streaming, daily independent releases)
    for t in audius_res {
        if seen_ids.insert(t.id.clone()) {
            combined.push(t);
        }
    }

    let max_len = sc_filtered.len().max(yt_filtered.len());
    for i in 0..max_len {
        if i < sc_filtered.len() {
            let t = &sc_filtered[i];
            if seen_ids.insert(t.id.clone()) {
                combined.push(t.clone());
            }
        }
        if i < yt_filtered.len() {
            let t = &yt_filtered[i];
            if seen_ids.insert(t.id.clone()) {
                combined.push(t.clone());
            }
        }
    }

    if combined.is_empty() && !is_cloud_env() {
        let cloud_tracks = execute_cloud_search(query, &base_url).await;
        for t in cloud_tracks {
            if seen_ids.insert(t.id.clone()) {
                combined.push(t);
            }
        }
    }

    Ok(Json(combined))
}

fn extract_video_id(url: &str) -> Option<String> {
    if let Some(idx) = url.find("v=") {
        let rest = &url[idx + 2..];
        let end = rest.find(['&', '?', '#', '/']).unwrap_or(rest.len());
        let id = &rest[..end];
        if !id.is_empty() && id.len() <= 20 {
            return Some(id.to_string());
        }
    }
    if let Some(idx) = url.find("youtu.be/") {
        let rest = &url[idx + 9..];
        let end = rest.find(['&', '?', '#', '/']).unwrap_or(rest.len());
        let id = &rest[..end];
        if !id.is_empty() && id.len() <= 20 {
            return Some(id.to_string());
        }
    }
    None
}

pub async fn extract_info(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(params): Query<ExtractParams>,
) -> Result<Json<ExtractResponse>, StatusCode> {
    let client_ip = get_client_ip(&headers, Some(addr));
    check_rate_limit(
        &state.endpoint_rate_limits,
        &format!("extract:{}", client_ip),
        25,
        60,
    )?;

    let url = params.url.trim();
    if url.is_empty() || url.starts_with('-') {
        return Err(StatusCode::BAD_REQUEST);
    }

    if url.starts_with("http://") || url.starts_with("https://") {
        let is_trusted = url.contains("youtube.com")
            || url.contains("youtu.be")
            || url.contains("soundcloud.com");
        if !is_trusted {
            check_url_ssrf(url).await?;
        }
    }

    let _permit = match tokio::time::timeout(
        Duration::from_millis(2000),
        state.heavy_process_semaphore.acquire(),
    )
    .await
    {
        Ok(Ok(permit)) => permit,
        _ => return Err(StatusCode::TOO_MANY_REQUESTS),
    };

    let yt_cmd = get_yt_dlp_cmd();
    let base_url = get_base_url();
    let is_radio_mix = url.contains("list=RD") || url.contains("list=UL");
    let video_id_opt = extract_video_id(url);

    let mut main_video: Option<SearchTrack> = None;
    let mut chapter_tracks: Vec<SearchTrack> = Vec::new();
    let mut has_chapters = false;

    if let Some(ref vid) = video_id_opt {
        let single_url = format!("https://www.youtube.com/watch?v={}", vid);
        let mut single_cmd = Command::new(&yt_cmd);
        apply_yt_dlp_common_args(&mut single_cmd);
        single_cmd.args([
            &single_url,
            "--dump-json",
            "--no-playlist",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

        if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(10), single_cmd.output()).await {
            if out.status.success() {
                if let Ok(item) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
                    main_video = parse_track_json(&item, &base_url);

                    if let Some(chapters) = item["chapters"].as_array() {
                        if !chapters.is_empty() {
                            for (i, ch) in chapters.iter().enumerate() {
                                let ch_title = ch["title"].as_str().unwrap_or("Без названия").trim();
                                let start_time = ch["start_time"].as_f64().unwrap_or(0.0);
                                let end_time = ch["end_time"].as_f64().unwrap_or(start_time);
                                let ch_duration = if end_time > start_time { end_time - start_time } else { 0.0 };

                                let ch_id = format!("{}_ch_{}", vid, i + 1);
                                let ch_artist = main_video.as_ref().map(|m| m.artist.clone()).unwrap_or_else(|| "Разные исполнители".to_string());
                                let ch_audio_url = format!("{}/api/stream?url={}&ss={}&title={}&artist={}", base_url, urlencoding::encode(&single_url), start_time as u64, urlencoding::encode(ch_title), urlencoding::encode(&ch_artist));
                                let ch_cover = main_video.as_ref().and_then(|m| m.cover_url.clone());

                                chapter_tracks.push(SearchTrack {
                                    id: ch_id,
                                    title: ch_title.to_string(),
                                    artist: ch_artist,
                                    duration: ch_duration,
                                    audio_url: ch_audio_url,
                                    cover_url: ch_cover,
                                });
                            }
                            if !chapter_tracks.is_empty() {
                                has_chapters = true;
                            }
                        }
                    }
                }
            }
        }
    }

    if has_chapters && !chapter_tracks.is_empty() {
        let playlist_title = main_video.as_ref().map(|m| m.title.clone());
        return Ok(Json(ExtractResponse {
            playlist_title,
            tracks: chapter_tracks,
            main_video,
            is_radio_mix,
            has_chapters: true,
        }));
    }

    let mut tracks = Vec::new();
    let mut playlist_title = None;

    let mut cmd = Command::new(&yt_cmd);
    apply_yt_dlp_common_args(&mut cmd);
    cmd.args([
        "--dump-json",
        "--flat-playlist",
        "--playlist-end",
        "100",
        "--",
        url,
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::null());

    if let Ok(mut child) = cmd.spawn() {
        if let Some(stdout) = child.stdout.take() {
            let mut reader = tokio::io::BufReader::new(stdout).lines();
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
            let _ = tokio::time::timeout(Duration::from_secs(25), read_task).await;
            let _ = child.kill().await;
        }
    }

    if tracks.is_empty() {
        if let Some(ref mv) = main_video {
            tracks.push(mv.clone());
        }
    }

    if tracks.is_empty() && !is_cloud_env() {
        let cloud_url = format!("{}/api/extract?url={}", CLOUD_FALLBACK_URL, urlencoding::encode(url));
        if let Ok(client) = reqwest::Client::builder().timeout(Duration::from_secs(15)).build() {
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
                            if let Some(ref mut mv) = ext_resp.main_video {
                                if mv.audio_url.contains("/api/stream") {
                                    let stream_idx = mv.audio_url.find("/api/stream").unwrap();
                                    mv.audio_url = format!("{}{}", base_url, &mv.audio_url[stream_idx..]);
                                }
                            }
                            return Ok(Json(ext_resp));
                        }
                    }
                }
            }
        }
    }

    if tracks.is_empty() && main_video.is_none() {
        if let Some(ref vid) = video_id_opt {
            let oembed_url = format!(
                "https://www.youtube.com/oembed?url={}&format=json",
                urlencoding::encode(&format!("https://www.youtube.com/watch?v={}", vid))
            );
            if let Ok(client) = reqwest::Client::builder().timeout(Duration::from_secs(5)).build() {
                if let Ok(resp) = client.get(&oembed_url).send().await {
                    if resp.status().is_success() {
                        if let Ok(bytes) = resp.bytes().await {
                            if let Ok(oembed) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                                let raw_title = oembed["title"].as_str().unwrap_or("YouTube Track");
                                let raw_author = oembed["author_name"].as_str().unwrap_or("YouTube Artist");
                                let clean_artist = raw_author.replace(" - Topic", "");
                                let cover = oembed["thumbnail_url"].as_str().map(|u| {
                                    format!("{}/api/cover?url={}", base_url, urlencoding::encode(u))
                                });
                                let full_url = format!("https://www.youtube.com/watch?v={}", vid);
                                let audio_url = format!(
                                    "{}/api/stream?url={}&title={}&artist={}",
                                    base_url,
                                    urlencoding::encode(&full_url),
                                    urlencoding::encode(raw_title),
                                    urlencoding::encode(&clean_artist)
                                );
                                let fallback_track = SearchTrack {
                                    id: vid.clone(),
                                    title: raw_title.to_string(),
                                    artist: clean_artist,
                                    duration: 0.0,
                                    audio_url,
                                    cover_url: cover,
                                };
                                main_video = Some(fallback_track.clone());
                                tracks.push(fallback_track);
                            }
                        }
                    }
                }
            }
        }
    }

    if main_video.is_none() && !tracks.is_empty() {
        if let Some(ref vid) = video_id_opt {
            if let Some(found) = tracks.iter().find(|t| t.id == *vid) {
                main_video = Some(found.clone());
            }
        }
        if main_video.is_none() && !url.contains("list=") {
            main_video = tracks.first().cloned();
        }
    }

    Ok(Json(ExtractResponse {
        playlist_title,
        tracks,
        main_video,
        is_radio_mix,
        has_chapters,
    }))
}

pub async fn execute_audius_search(query: &str, limit: usize, base_url: &str) -> Vec<SearchTrack> {
    let url = format!(
        "https://discoveryprovider.audius.co/v1/tracks/search?query={}&app_name=RECRO_MUSIC&limit={}",
        urlencoding::encode(query),
        limit
    );
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(2200))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36")
        .build()
    {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let resp = match client.get(&url).send().await {
        Ok(r) if r.status().is_success() => r,
        _ => return Vec::new(),
    };

    let data: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut tracks = Vec::new();
    if let Some(items) = data["data"].as_array() {
        for item in items {
            let id = match item["id"].as_str() {
                Some(s) if !s.is_empty() => s.to_string(),
                _ => continue,
            };
            let title = item["title"].as_str().unwrap_or("Untitled").trim().to_string();
            let artist = item["user"]["name"].as_str()
                .or_else(|| item["user"]["handle"].as_str())
                .unwrap_or("Audius Artist").trim().to_string();
            let duration = item["duration"].as_f64().unwrap_or(0.0);

            if duration > 600.0 || (duration > 0.0 && duration < 30.0) {
                continue;
            }

            let cover_url = item["artwork"]["480x480"].as_str()
                .or_else(|| item["artwork"]["150x150"].as_str())
                .map(|u| u.to_string());

            let direct_stream_url = format!("https://discoveryprovider.audius.co/v1/tracks/{}/stream?app_name=RECRO_MUSIC", id);
            let encoded_url = urlencoding::encode(&direct_stream_url);
            let encoded_title = urlencoding::encode(&title);
            let encoded_artist = urlencoding::encode(&artist);
            let audio_url = format!("{}/api/stream?url={}&title={}&artist={}", base_url, encoded_url, encoded_title, encoded_artist);

            tracks.push(SearchTrack {
                id: format!("audius-{}", id),
                title,
                artist,
                duration,
                audio_url,
                cover_url,
            });
        }
    }
    tracks
}
