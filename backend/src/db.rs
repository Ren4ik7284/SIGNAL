use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Pool, Sqlite};
use std::str::FromStr;

pub type DbPool = Pool<Sqlite>;

pub async fn init_db() -> Result<DbPool, sqlx::Error> {
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://signal.db?mode=rwc".to_string());

    let connection_options = SqliteConnectOptions::from_str(&db_url)?
        .create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(connection_options)
        .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            username TEXT UNIQUE NOT NULL,
            email TEXT UNIQUE,
            password_hash TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        "#,
    )
    .execute(&pool)
    .await?;

    let _ = sqlx::query("ALTER TABLE users ADD COLUMN email TEXT").execute(&pool).await;
    let _ = sqlx::query("CREATE UNIQUE INDEX IF NOT EXISTS idx_users_email ON users(email)").execute(&pool).await;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS email_verifications (
            email TEXT PRIMARY KEY,
            code TEXT NOT NULL,
            username TEXT NOT NULL,
            password_hash TEXT NOT NULL,
            expires_at INTEGER NOT NULL,
            created_at INTEGER NOT NULL,
            attempts INTEGER DEFAULT 0
        );
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS tracks (
            id TEXT NOT NULL,
            user_id TEXT NOT NULL,
            title TEXT NOT NULL,
            artist TEXT NOT NULL,
            album TEXT,
            duration REAL NOT NULL,
            audio_url TEXT NOT NULL,
            cover_url TEXT,
            genre TEXT,
            format TEXT,
            bitrate TEXT,
            plays INTEGER DEFAULT 0,
            is_favorite INTEGER DEFAULT 0,
            is_live_stream INTEGER DEFAULT 0,
            is_local_upload INTEGER DEFAULT 0,
            playlist_only INTEGER DEFAULT 0,
            added_at TEXT,
            PRIMARY KEY (user_id, id)
        );
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS playlists (
            id TEXT NOT NULL,
            user_id TEXT NOT NULL,
            title TEXT NOT NULL,
            description TEXT,
            cover_text TEXT,
            track_ids TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            PRIMARY KEY (user_id, id)
        );
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS radio_stations (
            id TEXT NOT NULL,
            user_id TEXT NOT NULL,
            name TEXT NOT NULL,
            stream_url TEXT NOT NULL,
            genre TEXT,
            country TEXT,
            bitrate TEXT,
            favicon TEXT,
            is_custom INTEGER DEFAULT 1,
            PRIMARY KEY (user_id, id)
        );
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS listening_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id TEXT NOT NULL,
            track_id TEXT NOT NULL,
            track_title TEXT NOT NULL,
            track_artist TEXT NOT NULL,
            track_genre TEXT,
            cover_url TEXT,
            duration REAL,
            played_at INTEGER NOT NULL
        );
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS user_sync_meta (
            user_id TEXT PRIMARY KEY,
            updated_at INTEGER NOT NULL
        );
        "#,
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}
