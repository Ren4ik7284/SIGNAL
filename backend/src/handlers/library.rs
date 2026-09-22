use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::{json, Value};
use sqlx::Row;

use crate::auth::extract_claims_from_headers;
use crate::AppState;

/// GET /api/sync — возвращает библиотеку текущего пользователя.
/// Требует Authorization: Bearer <token>.
/// Анонимные запросы получают 401 — никаких shared library.json!
pub async fn get_library(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    let claims = extract_claims_from_headers(&headers)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    let user_id = claims.sub;

    let meta_row = sqlx::query("SELECT updated_at FROM user_sync_meta WHERE user_id = ?")
        .bind(&user_id)
        .fetch_optional(&state.pool)
        .await
        .unwrap_or(None);

    let updated_at: i64 = meta_row.and_then(|r| r.try_get("updated_at").ok()).unwrap_or(0);

    let track_rows = sqlx::query(
        r#"
        SELECT id, title, artist, album, duration, audio_url, cover_url, genre, format, bitrate,
               plays, is_favorite, is_live_stream, is_local_upload, playlist_only, added_at
        FROM tracks
        WHERE user_id = ?
        ORDER BY rowid DESC
        "#,
    )
    .bind(&user_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut tracks = Vec::new();
    for r in track_rows {
        let id: String = r.get("id");
        let title: String = r.get("title");
        let artist: String = r.get("artist");
        let album: Option<String> = r.get("album");
        let duration: f64 = r.get("duration");
        let audio_url: String = r.get("audio_url");
        let cover_url: Option<String> = r.get("cover_url");
        let genre: Option<String> = r.get("genre");
        let format: Option<String> = r.get("format");
        let bitrate: Option<String> = r.get("bitrate");
        let plays: i64 = r.get("plays");
        let is_favorite: i64 = r.get("is_favorite");
        let is_live_stream: i64 = r.get("is_live_stream");
        let is_local_upload: i64 = r.get("is_local_upload");
        let playlist_only: i64 = r.get("playlist_only");
        let added_at: Option<String> = r.get("added_at");

        tracks.push(json!({
            "id": id,
            "title": title,
            "artist": artist,
            "album": album,
            "duration": duration,
            "audioUrl": audio_url,
            "coverUrl": cover_url,
            "genre": genre.unwrap_or_else(|| "Music".to_string()),
            "format": format.unwrap_or_else(|| "mp3".to_string()),
            "bitrate": bitrate,
            "plays": plays,
            "isFavorite": is_favorite == 1,
            "isLiveStream": is_live_stream == 1,
            "isLocalUpload": is_local_upload == 1,
            "playlistOnly": playlist_only == 1,
            "addedAt": added_at.unwrap_or_default(),
        }));
    }

    let playlist_rows = sqlx::query(
        r#"
        SELECT id, title, description, cover_text, track_ids
        FROM playlists
        WHERE user_id = ?
        "#,
    )
    .bind(&user_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut playlists = Vec::new();
    for r in playlist_rows {
        let id: String = r.get("id");
        let title: String = r.get("title");
        let description: Option<String> = r.get("description");
        let cover_text: Option<String> = r.get("cover_text");
        let track_ids_raw: String = r.get("track_ids");
        let track_ids: Vec<String> = serde_json::from_str(&track_ids_raw).unwrap_or_default();

        playlists.push(json!({
            "id": id,
            "title": title,
            "description": description.unwrap_or_default(),
            "coverText": cover_text.unwrap_or_else(|| "PL".to_string()),
            "trackIds": track_ids,
        }));
    }

    let station_rows = sqlx::query(
        r#"
        SELECT id, name, stream_url, genre, country, bitrate, favicon, is_custom
        FROM radio_stations
        WHERE user_id = ?
        "#,
    )
    .bind(&user_id)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut radio_stations = Vec::new();
    for r in station_rows {
        let id: String = r.get("id");
        let name: String = r.get("name");
        let stream_url: String = r.get("stream_url");
        let genre: Option<String> = r.get("genre");
        let country: Option<String> = r.get("country");
        let bitrate: Option<String> = r.get("bitrate");
        let favicon: Option<String> = r.get("favicon");
        let is_custom: i64 = r.get("is_custom");

        radio_stations.push(json!({
            "id": id,
            "name": name,
            "streamUrl": stream_url,
            "genre": genre.unwrap_or_else(|| "Radio".to_string()),
            "country": country,
            "bitrate": bitrate,
            "favicon": favicon,
            "isCustom": is_custom == 1,
        }));
    }

    Ok(Json(json!({
        "updated_at": updated_at,
        "tracks": tracks,
        "playlists": playlists,
        "radio_stations": radio_stations,
    })))
}

/// POST /api/sync — сохраняет библиотеку пользователя.
/// Требует Authorization: Bearer <token>.
/// Использует транзакцию — данные либо сохраняются полностью, либо не сохраняются вовсе.
pub async fn save_library(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(data): Json<Value>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    let claims = extract_claims_from_headers(&headers)
        .map_err(|(code, msg)| (code, Json(json!({ "error": msg }))))?;
    let user_id = claims.sub;

    let updated_at = data
        .get("updated_at")
        .and_then(|v| v.as_i64())
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());

    // Открываем транзакцию — либо всё сохраняется, либо ничего
    let mut tx = state.pool.begin().await.map_err(|e| {
        eprintln!("[SIGNAL SYNC] Ошибка начала транзакции: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Ошибка базы данных" })),
        )
    })?;

    // Обновляем метаданные синка
    sqlx::query(
        r#"
        INSERT INTO user_sync_meta (user_id, updated_at)
        VALUES (?, ?)
        ON CONFLICT(user_id) DO UPDATE SET updated_at = excluded.updated_at
        "#,
    )
    .bind(&user_id)
    .bind(updated_at)
    .execute(&mut *tx)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Ошибка обновления метаданных" })),
        )
    })?;

    // Сохраняем треки
    if let Some(tracks_arr) = data.get("tracks").and_then(|v| v.as_array()) {
        sqlx::query("DELETE FROM tracks WHERE user_id = ?")
            .bind(&user_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "Ошибка очистки треков" })),
                )
            })?;

        for item in tracks_arr {
            let id = item.get("id").and_then(|v| v.as_str()).unwrap_or_default();
            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or_default();
            let artist = item.get("artist").and_then(|v| v.as_str()).unwrap_or_default();
            let album = item.get("album").and_then(|v| v.as_str());
            let duration = item.get("duration").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let audio_url = item.get("audioUrl").and_then(|v| v.as_str()).unwrap_or_default();
            let cover_url = item.get("coverUrl").and_then(|v| v.as_str());
            let genre = item.get("genre").and_then(|v| v.as_str()).unwrap_or("Music");
            let format = item.get("format").and_then(|v| v.as_str()).unwrap_or("mp3");
            let bitrate = item.get("bitrate").and_then(|v| v.as_str());
            let plays = item.get("plays").and_then(|v| v.as_i64()).unwrap_or(0);
            let is_fav = if item.get("isFavorite").and_then(|v| v.as_bool()).unwrap_or(false) { 1 } else { 0 };
            let is_live = if item.get("isLiveStream").and_then(|v| v.as_bool()).unwrap_or(false) { 1 } else { 0 };
            let is_upload = if item.get("isLocalUpload").and_then(|v| v.as_bool()).unwrap_or(false) { 1 } else { 0 };
            let is_pl_only = if item.get("playlistOnly").and_then(|v| v.as_bool()).unwrap_or(false) { 1 } else { 0 };
            let added_at = item.get("addedAt").and_then(|v| v.as_str()).unwrap_or_default();

            if !id.is_empty() && !audio_url.is_empty() {
                sqlx::query(
                    r#"
                    INSERT OR REPLACE INTO tracks
                    (id, user_id, title, artist, album, duration, audio_url, cover_url, genre, format, bitrate, plays, is_favorite, is_live_stream, is_local_upload, playlist_only, added_at)
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                    "#,
                )
                .bind(id)
                .bind(&user_id)
                .bind(title)
                .bind(artist)
                .bind(album)
                .bind(duration)
                .bind(audio_url)
                .bind(cover_url)
                .bind(genre)
                .bind(format)
                .bind(bitrate)
                .bind(plays)
                .bind(is_fav)
                .bind(is_live)
                .bind(is_upload)
                .bind(is_pl_only)
                .bind(added_at)
                .execute(&mut *tx)
                .await
                .map_err(|e| {
                    eprintln!("[SIGNAL SYNC] Ошибка вставки трека {}: {}", id, e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": "Ошибка сохранения трека" })),
                    )
                })?;
            }
        }
    }

    // Сохраняем плейлисты
    if let Some(pl_arr) = data.get("playlists").and_then(|v| v.as_array()) {
        sqlx::query("DELETE FROM playlists WHERE user_id = ?")
            .bind(&user_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "Ошибка очистки плейлистов" })),
                )
            })?;

        for item in pl_arr {
            let id = item.get("id").and_then(|v| v.as_str()).unwrap_or_default();
            let title = item.get("title").and_then(|v| v.as_str()).unwrap_or_default();
            let desc = item.get("description").and_then(|v| v.as_str()).unwrap_or_default();
            let cover_text = item.get("coverText").and_then(|v| v.as_str()).unwrap_or("PL");
            let track_ids = item.get("trackIds").map(|v| v.to_string()).unwrap_or_else(|| "[]".to_string());
            let now = chrono::Utc::now().timestamp();

            if !id.is_empty() {
                sqlx::query(
                    r#"
                    INSERT OR REPLACE INTO playlists (id, user_id, title, description, cover_text, track_ids, created_at)
                    VALUES (?, ?, ?, ?, ?, ?, ?)
                    "#,
                )
                .bind(id)
                .bind(&user_id)
                .bind(title)
                .bind(desc)
                .bind(cover_text)
                .bind(track_ids)
                .bind(now)
                .execute(&mut *tx)
                .await
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": "Ошибка сохранения плейлиста" })),
                    )
                })?;
            }
        }
    }

    // Сохраняем радиостанции
    if let Some(st_arr) = data.get("radio_stations").and_then(|v| v.as_array()) {
        sqlx::query("DELETE FROM radio_stations WHERE user_id = ?")
            .bind(&user_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": "Ошибка очистки радиостанций" })),
                )
            })?;

        for item in st_arr {
            let id = item.get("id").and_then(|v| v.as_str()).unwrap_or_default();
            let name = item.get("name").and_then(|v| v.as_str()).unwrap_or_default();
            let stream_url = item.get("streamUrl").and_then(|v| v.as_str()).unwrap_or_default();
            let genre = item.get("genre").and_then(|v| v.as_str());
            let country = item.get("country").and_then(|v| v.as_str());
            let bitrate = item.get("bitrate").and_then(|v| v.as_str());
            let favicon = item.get("favicon").and_then(|v| v.as_str());
            let is_custom = if item.get("isCustom").and_then(|v| v.as_bool()).unwrap_or(true) { 1 } else { 0 };

            if !id.is_empty() && !stream_url.is_empty() {
                sqlx::query(
                    r#"
                    INSERT OR REPLACE INTO radio_stations (id, user_id, name, stream_url, genre, country, bitrate, favicon, is_custom)
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                    "#,
                )
                .bind(id)
                .bind(&user_id)
                .bind(name)
                .bind(stream_url)
                .bind(genre)
                .bind(country)
                .bind(bitrate)
                .bind(favicon)
                .bind(is_custom)
                .execute(&mut *tx)
                .await
                .map_err(|_| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": "Ошибка сохранения радиостанции" })),
                    )
                })?;
            }
        }
    }

    // Фиксируем транзакцию — только теперь данные реально сохранены
    tx.commit().await.map_err(|e| {
        eprintln!("[SIGNAL SYNC] Ошибка коммита транзакции: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Ошибка сохранения библиотеки" })),
        )
    })?;

    Ok(StatusCode::OK)
}
