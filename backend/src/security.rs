use axum::http::{HeaderMap, StatusCode};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub fn is_private_or_restricted_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || ((v6.segments()[0] & 0xfe00) == 0xfc00)
                || ((v6.segments()[0] & 0xffc0) == 0xfe80)
        }
    }
}

pub async fn check_url_ssrf(url_str: &str) -> Result<(), StatusCode> {
    let parsed_url = reqwest::Url::parse(url_str).map_err(|_| StatusCode::BAD_REQUEST)?;
    if parsed_url.scheme() != "http" && parsed_url.scheme() != "https" {
        return Err(StatusCode::BAD_REQUEST);
    }

    let host = parsed_url.host_str().ok_or(StatusCode::BAD_REQUEST)?;
    let lower_host = host.to_lowercase();
    if lower_host == "localhost"
        || lower_host.ends_with(".local")
        || lower_host.ends_with(".internal")
        || lower_host.ends_with(".lan")
        || lower_host == "127.0.0.1"
        || lower_host == "::1"
    {
        return Err(StatusCode::FORBIDDEN);
    }

    let port = parsed_url.port_or_known_default().unwrap_or(80);
    let lookup_addr = format!("{}:{}", host, port);
    let addrs = tokio::net::lookup_host(&lookup_addr)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let mut resolved = false;
    for addr in addrs {
        resolved = true;
        if is_private_or_restricted_ip(addr.ip()) {
            return Err(StatusCode::FORBIDDEN);
        }
    }

    if !resolved {
        return Err(StatusCode::BAD_REQUEST);
    }

    Ok(())
}

pub fn get_client_ip(
    headers: &HeaderMap,
    socket_addr: Option<SocketAddr>,
) -> String {
    let trust_proxy = std::env::var("TRUST_PROXY")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    if trust_proxy {
        if let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
            if let Some(first_ip) = forwarded.split(',').next() {
                let trimmed = first_ip.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
        }
        if let Some(real_ip) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
            let trimmed = real_ip.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }

    if let Some(sa) = socket_addr {
        return sa.ip().to_string();
    }

    "127.0.0.1".to_string()
}

pub fn check_rate_limit(
    rate_limits: &Arc<Mutex<HashMap<String, (u32, Instant)>>>,
    key: &str,
    max_requests: u32,
    window_secs: u64,
) -> Result<(), StatusCode> {
    let mut map = rate_limits.lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    let window = Duration::from_secs(window_secs);

    if map.len() > 3000 {
        map.retain(|_, (_, start)| now.duration_since(*start) <= window);
    }

    let entry = map.entry(key.to_string()).or_insert((0, now));
    if now.duration_since(entry.1) > window {
        *entry = (1, now);
        Ok(())
    } else {
        entry.0 += 1;
        if entry.0 > max_requests {
            Err(StatusCode::TOO_MANY_REQUESTS)
        } else {
            Ok(())
        }
    }
}
