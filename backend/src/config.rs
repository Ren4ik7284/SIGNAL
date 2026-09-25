use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

pub const CLOUD_FALLBACK_URL: &str = "https://signal-audio-backend-production.up.railway.app";

pub fn is_cloud_env() -> bool {
    std::env::var("RAILWAY_PUBLIC_DOMAIN").is_ok()
}

pub fn get_yt_dlp_cmd() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let local_path = format!("{}/.local/bin/yt-dlp", home);
    if Path::new(&local_path).exists() {
        local_path
    } else {
        "yt-dlp".to_string()
    }
}

pub fn init_cookies_from_env() {
    if let Ok(content) = std::env::var("YT_COOKIES") {
        let trimmed = content.trim();
        if !trimmed.is_empty() {
            let target = if Path::new("/data").exists() {
                "/data/cookies.txt"
            } else if Path::new("/app").exists() {
                "/app/cookies.txt"
            } else if Path::new("backend").exists() {
                "backend/cookies.txt"
            } else {
                "cookies.txt"
            };
            let _ = std::fs::write(target, trimmed.as_bytes());
        }
    }
}

pub fn get_cookies_path() -> Option<String> {
    if let Ok(env_path) = std::env::var("YT_COOKIES_PATH") {
        if Path::new(&env_path).exists() {
            return Some(env_path);
        }
    }
    if Path::new("/data/cookies.txt").exists() {
        return Some("/data/cookies.txt".to_string());
    }
    if Path::new("/app/cookies.txt").exists() {
        return Some("/app/cookies.txt".to_string());
    }
    if Path::new("/tmp/cookies.txt").exists() {
        return Some("/tmp/cookies.txt".to_string());
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

pub fn has_chromium_profile() -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    Path::new(&format!("{}/.config/chromium", home)).exists()
        || Path::new(&format!("{}/.config/google-chrome", home)).exists()
}

pub fn apply_yt_dlp_common_args(cmd: &mut Command) {
    cmd.stdin(Stdio::null());
    cmd.stderr(Stdio::inherit());
    cmd.args([
        "--no-warnings",
    ]);
    if let Ok(proxy) = std::env::var("YOUTUBE_PROXY") {
        let p = proxy.trim();
        if !p.is_empty() {
            cmd.arg("--proxy").arg(p);
        }
    }
    if let Ok(env_cookies) = std::env::var("YT_COOKIES_PATH") {
        if Path::new(&env_cookies).exists() {
            cmd.arg("--cookies").arg(env_cookies);
            return;
        }
    }
    if let Some(cookies) = get_cookies_path() {
        cmd.arg("--cookies").arg(cookies);
    }
}

pub fn apply_yt_dlp_common_args_no_cookies(cmd: &mut Command) {
    cmd.stdin(Stdio::null());
    cmd.stderr(Stdio::inherit());
    cmd.args([
        "--no-warnings",
    ]);
    if let Ok(proxy) = std::env::var("YOUTUBE_PROXY") {
        let p = proxy.trim();
        if !p.is_empty() {
            cmd.arg("--proxy").arg(p);
        }
    }
}

pub async fn ensure_cookies_on_start() {
    if !has_chromium_profile() {
        return;
    }
    let yt_cmd = get_yt_dlp_cmd();
    let home = std::env::var("HOME").unwrap_or_default();
    let cookies_path = format!("{}/music-player/backend/cookies.txt", home);
    let _ = Command::new(&yt_cmd)
        .args([
            "--cookies",
            &cookies_path,
            "--cookies-from-browser",
            "chromium",
            "--playlist-items",
            "0",
            "--skip-download",
            "https://www.youtube.com/watch?v=3YZ5yuDByQg",
        ])
        .output()
        .await;
}

pub fn get_base_url() -> String {
    if let Ok(domain) = std::env::var("RAILWAY_PUBLIC_DOMAIN") {
        return format!("https://{}", domain);
    }
    let port = std::env::var("PORT").unwrap_or_else(|_| "8085".to_string());
    format!("http://localhost:{}", port)
}
