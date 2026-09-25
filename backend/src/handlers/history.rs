use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;

use crate::auth::extract_claims_from_headers;
use crate::security::check_rate_limit;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct RecordPlayRequest {
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub genre: Option<String>,
    pub cover_url: Option<String>,
    pub duration: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct HistoryItem {
    pub id: i64,
    pub track_id: String,
    pub track_title: String,
    pub track_artist: String,
    pub track_genre: Option<String>,
    pub cover_url: Option<String>,
    pub duration: Option<f64>,
    pub played_at: i64,
}

pub async fn record_play(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RecordPlayRequest>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    let claims = extract_claims_from_headers(&headers).map_err(|(code, msg)| {
        (code, Json(json!({ "error": msg })))
    })?;

    check_rate_limit(
        &state.endpoint_rate_limits,
        &format!("history_record:{}", claims.sub),
        60,
        60,
    )
    .map_err(|c| (c, Json(json!({ "error": "Превышен лимит запросов истории" }))))?;

    let played_at = chrono::Utc::now().timestamp();

    sqlx::query(
        r#"
        INSERT INTO listening_history (user_id, track_id, track_title, track_artist, track_genre, cover_url, duration, played_at)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&claims.sub)
    .bind(&payload.track_id)
    .bind(&payload.title)
    .bind(&payload.artist)
    .bind(&payload.genre)
    .bind(&payload.cover_url)
    .bind(payload.duration)
    .bind(played_at)
    .execute(&state.pool)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Не удалось сохранить историю прослушивания" })),
        )
    })?;

    let _ = sqlx::query("UPDATE tracks SET plays = plays + 1 WHERE user_id = ? AND id = ?")
        .bind(&claims.sub)
        .bind(&payload.track_id)
        .execute(&state.pool)
        .await;

    let _ = sqlx::query(
        r#"
        DELETE FROM listening_history
        WHERE user_id = ? AND id NOT IN (
            SELECT id FROM listening_history
            WHERE user_id = ?
            ORDER BY played_at DESC
            LIMIT 500
        )
        "#,
    )
    .bind(&claims.sub)
    .bind(&claims.sub)
    .execute(&state.pool)
    .await;

    Ok(StatusCode::OK)
}

pub async fn get_history(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<HistoryItem>>, (StatusCode, Json<Value>)> {
    let claims = extract_claims_from_headers(&headers).map_err(|(code, msg)| {
        (code, Json(json!({ "error": msg })))
    })?;

    let rows = sqlx::query(
        r#"
        SELECT id, track_id, track_title, track_artist, track_genre, cover_url, duration, played_at
        FROM listening_history
        WHERE user_id = ?
        ORDER BY played_at DESC
        LIMIT 60
        "#,
    )
    .bind(&claims.sub)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Не удалось получить историю" })),
        )
    })?;

    let mut items = Vec::new();
    for row in rows {
        items.push(HistoryItem {
            id: row.get("id"),
            track_id: row.get("track_id"),
            track_title: row.get("track_title"),
            track_artist: row.get("track_artist"),
            track_genre: row.get("track_genre"),
            cover_url: row.get("cover_url"),
            duration: row.get("duration"),
            played_at: row.get("played_at"),
        });
    }

    Ok(Json(items))
}

pub async fn clear_history(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    let claims = extract_claims_from_headers(&headers).map_err(|(code, msg)| {
        (code, Json(json!({ "error": msg })))
    })?;

    sqlx::query("DELETE FROM listening_history WHERE user_id = ?")
        .bind(&claims.sub)
        .execute(&state.pool)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Не удалось очистить историю" })),
            )
        })?;

    Ok(StatusCode::OK)
}
