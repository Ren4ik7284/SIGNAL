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

fn is_private_or_local_host(host: &str) -> bool {
    let lower = host.to_lowercase();
    if lower == "localhost" || lower.ends_with(".local") || lower.ends_with(".internal") {
        return true;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        match ip {
            IpAddr::V4(v4) => {
                v4.is_loopback()
                    || v4.is_private()
                    || v4.is_link_local()
                    || v4.is_broadcast()
                    || v4.is_unspecified()
            }
            IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified(),
        }
    } else {
        false
    }
}

pub async fn proxy_cover(Query(params): Query<CoverParams>) -> Result<Response, StatusCode> {
    let target = params.url.trim();
    if target.is_empty() || (!target.starts_with("http://") && !target.starts_with("https://")) {
        return Err(StatusCode::BAD_REQUEST);
    }

    let parsed_url = reqwest::Url::parse(target).map_err(|_| StatusCode::BAD_REQUEST)?;
    if let Some(host) = parsed_url.host_str() {
        if is_private_or_local_host(host) {
            return Err(StatusCode::FORBIDDEN);
        }
    } else {
        return Err(StatusCode::BAD_REQUEST);
    }

    let client = reqwest::Client::builder()
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
