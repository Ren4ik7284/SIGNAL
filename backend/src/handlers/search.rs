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

pub async fn extract_info(Query(params): Query<ExtractParams>) -> Result<Json<ExtractResponse>, StatusCode> {
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
