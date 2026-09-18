use axum::{http::StatusCode, Json};
use std::path::Path;

pub async fn get_library() -> Result<Json<serde_json::Value>, StatusCode> {
    if Path::new("library.json").exists() {
        if let Ok(content) = std::fs::read_to_string("library.json") {
            if let Ok(data) = serde_json::from_str::<serde_json::Value>(&content) {
                return Ok(Json(data));
            }
        }
    }
    Ok(Json(serde_json::json!({
        "tracks": [],
        "playlists": [],
        "radio_stations": []
    })))
}

pub async fn save_library(Json(data): Json<serde_json::Value>) -> Result<StatusCode, StatusCode> {
    if let Ok(json_str) = serde_json::to_string_pretty(&data) {
        if std::fs::write("library.json", json_str).is_ok() {
            return Ok(StatusCode::OK);
        }
    }
    Err(StatusCode::INTERNAL_SERVER_ERROR)
}
