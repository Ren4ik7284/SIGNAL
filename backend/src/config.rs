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

pub fn get_cookies_path() -> Option<String> {
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

pub fn has_chromium_profile() -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    Path::new(&format!("{}/.config/chromium", home)).exists()
        || Path::new(&format!("{}/.config/google-chrome", home)).exists()
}

pub fn apply_yt_dlp_common_args(cmd: &mut Command) {
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

pub async fn ensure_cookies_on_start() {
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

pub fn get_base_url() -> String {
    if let Ok(domain) = std::env::var("RAILWAY_PUBLIC_DOMAIN") {
        return format!("https://{}", domain);
    }
    let port = std::env::var("PORT").unwrap_or_else(|_| "8085".to_string());
    format!("http://localhost:{}", port)
}
