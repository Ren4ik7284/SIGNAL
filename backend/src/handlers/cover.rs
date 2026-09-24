use axum::{
    extract::Query,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use std::net::IpAddr;
use std::time::Duration;

use crate::models::CoverParams;

pub async fn health_check() -> &'static str {
    "SIGNAL // Rust Engine Online"
}

fn is_private_or_restricted_ip(ip: IpAddr) -> bool {
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
                || ((v6.segments()[0] & 0xfe00) == 0xfc00) // Unique local fc00::/7
                || ((v6.segments()[0] & 0xffc0) == 0xfe80) // Link local unicast fe80::/10
        }
    }
}

pub async fn proxy_cover(Query(params): Query<CoverParams>) -> Result<Response, StatusCode> {
    let target = params.url.trim();
    if target.is_empty() || (!target.starts_with("http://") && !target.starts_with("https://")) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let parsed_url = reqwest::Url::parse(target).map_err(|_| StatusCode::BAD_REQUEST)?;
    let host = parsed_url.host_str().ok_or(StatusCode::BAD_REQUEST)?;

    // Check host string blacklist
    let lower_host = host.to_lowercase();
    if lower_host == "localhost"
        || lower_host.ends_with(".local")
        || lower_host.ends_with(".internal")
        || lower_host.ends_with(".lan")
    {
        return Err(StatusCode::FORBIDDEN);
    }

    // Resolve DNS and ensure no resolved IP is private/loopback/link-local
    let port = parsed_url.port_or_known_default().unwrap_or(80);
    let lookup_addr = format!("{}:{}", host, port);
    match tokio::net::lookup_host(&lookup_addr).await {
        Ok(addrs) => {
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
        }
        Err(_) => return Err(StatusCode::BAD_REQUEST),
    }

    // Client without automatic redirects to prevent SSRF redirect bypass
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(6))
        .build()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let resp = client
        .get(target)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .send()
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY)?;

    // Limit download size to 10 MB to prevent DoS/OOM
    if let Some(len) = resp.content_length() {
        if len > 10 * 1024 * 1024 {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
    }

    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_string();

    let ct_lower = content_type.to_lowercase();
    if !ct_lower.starts_with("image/") && ct_lower != "application/octet-stream" {
        return Err(StatusCode::BAD_REQUEST);
    }

    let bytes = resp.bytes().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    if bytes.len() > 10 * 1024 * 1024 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }

    let mut res_headers = HeaderMap::new();
    let valid_ct = HeaderValue::from_str(&content_type)
        .unwrap_or_else(|_| HeaderValue::from_static("image/jpeg"));
    res_headers.insert(header::CONTENT_TYPE, valid_ct);
    res_headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=604800, immutable"),
    );
    res_headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));

    Ok((StatusCode::OK, res_headers, bytes).into_response())
}
