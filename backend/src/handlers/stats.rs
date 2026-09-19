use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::Row;

use crate::auth::extract_claims_from_headers;
use crate::AppState;

#[derive(Debug, Serialize)]
pub struct TopTrackItem {
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub cover_url: Option<String>,
    pub plays: i64,
}

#[derive(Debug, Serialize)]
pub struct TopArtistItem {
    pub artist: String,
    pub plays: i64,
}

#[derive(Debug, Serialize)]
pub struct TopGenreItem {
    pub genre: String,
    pub plays: i64,
    pub percentage: f64,
}

#[derive(Debug, Serialize)]
pub struct MusicPersonality {
    pub title: String,
    pub tag: String,
    pub description: String,
}

#[derive(Debug, Serialize)]
pub struct WrappedStats {
    pub username: String,
    pub total_plays: i64,
    pub total_minutes: i64,
    pub unique_tracks: i64,
    pub unique_artists: i64,
    pub top_tracks: Vec<TopTrackItem>,
    pub top_artists: Vec<TopArtistItem>,
    pub top_genres: Vec<TopGenreItem>,
    pub personality: MusicPersonality,
}

pub async fn get_wrapped(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WrappedStats>, (StatusCode, Json<Value>)> {
    let claims = extract_claims_from_headers(&headers).map_err(|(code, msg)| {
        (code, Json(json!({ "error": msg })))
    })?;

    let total_row = sqlx::query(
        r#"
        SELECT 
            COUNT(*) as total_plays,
            COALESCE(SUM(duration), 0) as total_duration_secs,
            COUNT(DISTINCT track_id) as unique_tracks,
            COUNT(DISTINCT track_artist) as unique_artists
        FROM listening_history
        WHERE user_id = ?
        "#,
    )
    .bind(&claims.sub)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Не удалось рассчитать общую статистику" })),
        )
    })?;

    let total_plays: i64 = total_row.get("total_plays");
    let total_duration_secs: f64 = total_row.get("total_duration_secs");
    let total_minutes = (total_duration_secs / 60.0).round() as i64;
    let unique_tracks: i64 = total_row.get("unique_tracks");
    let unique_artists: i64 = total_row.get("unique_artists");

    let track_rows = sqlx::query(
        r#"
        SELECT track_id, track_title, track_artist, cover_url, COUNT(*) as cnt
        FROM listening_history
        WHERE user_id = ?
        GROUP BY track_id, track_title, track_artist
        ORDER BY cnt DESC
        LIMIT 5
        "#,
    )
    .bind(&claims.sub)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut top_tracks = Vec::new();
    for r in track_rows {
        top_tracks.push(TopTrackItem {
            track_id: r.get("track_id"),
            title: r.get("track_title"),
            artist: r.get("track_artist"),
            cover_url: r.get("cover_url"),
            plays: r.get("cnt"),
        });
    }

    let artist_rows = sqlx::query(
        r#"
        SELECT track_artist, COUNT(*) as cnt
        FROM listening_history
        WHERE user_id = ? AND track_artist != ''
        GROUP BY track_artist
        ORDER BY cnt DESC
        LIMIT 5
        "#,
    )
    .bind(&claims.sub)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut top_artists = Vec::new();
    for r in artist_rows {
        top_artists.push(TopArtistItem {
            artist: r.get("track_artist"),
            plays: r.get("cnt"),
        });
    }

    let genre_rows = sqlx::query(
        r#"
        SELECT track_genre, COUNT(*) as cnt
        FROM listening_history
        WHERE user_id = ? AND track_genre IS NOT NULL AND track_genre != ''
        GROUP BY track_genre
        ORDER BY cnt DESC
        LIMIT 5
        "#,
    )
    .bind(&claims.sub)
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    let mut top_genres = Vec::new();
    let total_genre_plays: i64 = genre_rows.iter().map(|r| r.get::<i64, _>("cnt")).sum();

    for r in genre_rows {
        let cnt: i64 = r.get("cnt");
        let pct = if total_genre_plays > 0 {
            (cnt as f64 / total_genre_plays as f64) * 100.0
        } else {
            0.0
        };
        top_genres.push(TopGenreItem {
            genre: r.get("track_genre"),
            plays: cnt,
            percentage: (pct * 10.0).round() / 10.0,
        });
    }

    let primary_genre = top_genres.first().map(|g| g.genre.as_str()).unwrap_or("Разное");
    let personality = match primary_genre.to_lowercase().as_str() {
        g if g.contains("electron") || g.contains("synth") || g.contains("techno") => MusicPersonality {
            title: "Кибер-Архитектор".to_string(),
            tag: "#CYBER_SOUND".to_string(),
            description: "Ваш пульс синхронизирован с синтезаторами и цифровым потоком SIGNAL.".to_string(),
        },
        g if g.contains("ambient") || g.contains("chill") || g.contains("lounge") => MusicPersonality {
            title: "Космический Дрейфующий".to_string(),
            tag: "#DEEP_DRIFT".to_string(),
            description: "Глубокие текстуры и медитативное спокойствие определяют ваше музыкальное поле.".to_string(),
        },
        g if g.contains("hip") || g.contains("rap") || g.contains("trap") => MusicPersonality {
            title: "Ритмический Стратег".to_string(),
            tag: "#BEAT_DOMINATION".to_string(),
            description: "Вы цените панчи, бас и честный флоу без лишних компромиссов.".to_string(),
        },
        g if g.contains("rock") || g.contains("metal") => MusicPersonality {
            title: "Перегруженный Драйвер".to_string(),
            tag: "#HIGH_VOLTAGE".to_string(),
            description: "Максимальная энергия и чистый драйв звучат в каждом вашем треке.".to_string(),
        },
        _ => {
            if total_plays > 30 {
                MusicPersonality {
                    title: "Аудио-Сингулярность".to_string(),
                    tag: "#INFINITE_AUDIO".to_string(),
                    description: "Музыка звучит бесконечно, исследуя десятки различных жанров и миров.".to_string(),
                }
            } else {
                MusicPersonality {
                    title: "Исследователь Частот".to_string(),
                    tag: "#EXPLORER".to_string(),
                    description: "Вы только формируете свой звуковой почерк в экосистеме SIGNAL.".to_string(),
                }
            }
        }
    };

    Ok(Json(WrappedStats {
        username: claims.username,
        total_plays,
        total_minutes,
        unique_tracks,
        unique_artists,
        top_tracks,
        top_artists,
        top_genres,
        personality,
    }))
}
