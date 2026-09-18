use axum::{
    extract::Query,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use std::time::Duration;

use crate::models::CoverParams;

pub async fn health_check() -> &'static str {
    "SIGNAL // Rust Engine Online"
}

pub async fn proxy_cover(Query(params): Query<CoverParams>) -> Result<Response, StatusCode> {
    let target = params.url.trim();
    if target.is_empty() || (!target.starts_with("http://") && !target.starts_with("https://")) {
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

    let content_type = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_string();

    let bytes = resp.bytes().await.map_err(|_| StatusCode::BAD_GATEWAY)?;

    let mut res_headers = HeaderMap::new();
    res_headers.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
    res_headers.insert(
        header::CACHE_CONTROL,
        "public, max-age=604800, immutable".parse().unwrap(),
    );
    res_headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap());

    Ok((StatusCode::OK, res_headers, bytes).into_response())
}
