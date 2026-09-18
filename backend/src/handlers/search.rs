use axum::{extract::Query, http::StatusCode, Json};
use std::collections::HashSet;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

use crate::config::{get_base_url, get_yt_dlp_cmd, is_cloud_env, apply_yt_dlp_common_args, CLOUD_FALLBACK_URL};
use crate::models::{ExtractParams, ExtractResponse, SearchParams, SearchTrack};
use crate::ytdlp::{execute_cloud_search, execute_yt_dlp_search, parse_track_json};

pub async fn search_music(Query(params): Query<SearchParams>) -> Result<Json<Vec<SearchTrack>>, StatusCode> {
    let query = params.q.trim();
    if query.is_empty() {
        return Ok(Json(Vec::new()));
    }

    let yt_cmd = get_yt_dlp_cmd();
    let base_url = get_base_url();
    let is_direct_url = query.starts_with("http://") || query.starts_with("https://");

    println!("[search] Performing search for: {}", query);

    if is_direct_url {
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

pub async fn extract_info(Query(params): Query<ExtractParams>) -> Result<Json<ExtractResponse>, StatusCode> {
    let url = params.url.trim();
    if url.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let yt_cmd = get_yt_dlp_cmd();
    let base_url = get_base_url();
    let is_radio_mix = url.contains("list=RD") || url.contains("list=UL");
    let video_id_opt = extract_video_id(url);

    let mut main_video: Option<SearchTrack> = None;
    let mut chapter_tracks: Vec<SearchTrack> = Vec::new();
    let mut has_chapters = false;

    // 1. If URL has a specific video, inspect it first (fast single-video lookup)
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

                    // Check for video chapters (sub-tracks / timestamps)
                    if let Some(chapters) = item["chapters"].as_array() {
                        if !chapters.is_empty() {
                            for (i, ch) in chapters.iter().enumerate() {
                                let ch_title = ch["title"].as_str().unwrap_or("Без названия").trim();
                                let start_time = ch["start_time"].as_f64().unwrap_or(0.0);
                                let end_time = ch["end_time"].as_f64().unwrap_or(start_time);
                                let ch_duration = if end_time > start_time { end_time - start_time } else { 0.0 };

                                let ch_id = format!("{}_ch_{}", vid, i + 1);
                                let ch_audio_url = format!("{}/api/stream?url={}&ss={}", base_url, urlencoding::encode(&single_url), start_time as u64);
                                let ch_artist = main_video.as_ref().map(|m| m.artist.clone()).unwrap_or_else(|| "Разные исполнители".to_string());
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

    // 2. Determine tracks to return:
    // If video has chapters, chapter_tracks ARE the playlist tracks!
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

    // 3. Regular playlist extraction if URL has a playlist or if chapter extraction wasn't used
    let mut tracks = Vec::new();
    let mut playlist_title = None;

    let mut cmd = Command::new(&yt_cmd);
    apply_yt_dlp_common_args(&mut cmd);
    cmd.args([
        url,
        "--dump-json",
        "--flat-playlist",
        "--playlist-end",
        "100",
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

    // If tracks were empty but we have main_video (e.g. single video URL), use main_video as the track
    if tracks.is_empty() {
        if let Some(ref mv) = main_video {
            tracks.push(mv.clone());
        }
    }

    // Fallback to cloud extract if local extraction yielded 0 items
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

    // If main_video is still missing, but tracks has items and URL has a video_id, find it in tracks or use first track
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
