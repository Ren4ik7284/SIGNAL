use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Pool, Sqlite};
use std::str::FromStr;

pub type DbPool = Pool<Sqlite>;

pub async fn init_db() -> Result<DbPool, sqlx::Error> {
    let env_url = std::env::var("DATABASE_URL").unwrap_or_default();
    let db_url = if env_url.starts_with("sqlite:") {
        env_url
    } else if std::path::Path::new("/data").exists() {
        "sqlite:///data/signal.db?mode=rwc".to_string()
    } else {
        "sqlite://signal.db?mode=rwc".to_string()
    };

    println!("[SIGNAL DB] Использование базы данных: {}", db_url);

    let connection_options = SqliteConnectOptions::from_str(&db_url)?
        .create_if_missing(true)
        // WAL mode: параллельные читатели не блокируют писателя
        .journal_mode(SqliteJournalMode::Wal)
        // Ожидаем 5 сек вместо немедленного SQLITE_BUSY
        .pragma("busy_timeout", "5000")
        .pragma("synchronous", "NORMAL")
        .pragma("foreign_keys", "ON");

    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .connect_with(connection_options)
        .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            username TEXT UNIQUE NOT NULL,
            password_hash TEXT NOT NULL,
            google_id TEXT UNIQUE,
            email TEXT,
            avatar_url TEXT,
            created_at INTEGER NOT NULL
        );
        "#,
    )
    .execute(&pool)
    .await?;

    // Автоматическая миграция для существующих баз данных
    let _ = sqlx::query("ALTER TABLE users ADD COLUMN google_id TEXT").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE users ADD COLUMN email TEXT").execute(&pool).await;
    let _ = sqlx::query("ALTER TABLE users ADD COLUMN avatar_url TEXT").execute(&pool).await;
    let _ = sqlx::query("CREATE UNIQUE INDEX IF NOT EXISTS idx_users_google_id ON users(google_id) WHERE google_id IS NOT NULL").execute(&pool).await;
    let _ = sqlx::query("CREATE INDEX IF NOT EXISTS idx_users_email ON users(email)").execute(&pool).await;

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

    // Индексы для ускорения запросов истории и wrapped-stats
    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_history_user_played ON listening_history(user_id, played_at DESC)"
    ).execute(&pool).await;

    let _ = sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_tracks_user ON tracks(user_id)"
    ).execute(&pool).await;

    Ok(pool)
}
