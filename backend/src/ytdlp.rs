use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;

use crate::config::{apply_yt_dlp_common_args, CLOUD_FALLBACK_URL};
use crate::models::SearchTrack;

pub fn parse_track_json(item: &serde_json::Value, base_url: &str) -> Option<SearchTrack> {
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

pub async fn execute_yt_dlp_search(yt_cmd: &str, search_arg: &str, timeout_sec: u64, base_url: &str) -> Vec<SearchTrack> {
    let mut cmd = Command::new(yt_cmd);
    apply_yt_dlp_common_args(&mut cmd);
    cmd.args([
        search_arg,
        "--dump-json",
        "--flat-playlist",
        "--playlist-end",
        "50",
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

pub async fn execute_cloud_search(query: &str, base_url: &str) -> Vec<SearchTrack> {
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
