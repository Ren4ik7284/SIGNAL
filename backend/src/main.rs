use axum::{
    body::Body,
    extract::Query,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::Path;
use std::process::Stdio;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tokio_util::io::ReaderStream;
use tower_http::cors::CorsLayer;

// ============================================================================
// МОДЕЛИ ДАННЫХ (Data Structures)
//
// В Rust ключевое слово `struct` объявляет структуру данных (как `interface`
// в TypeScript или `class` с полями в Python / C#).
//
// Атрибуты `#[derive(Debug, Serialize, Deserialize)]` говорят компилятору
// автоматически сгенерировать код для:
// - `Debug`       -> отладочного вывода через println!("{:?}", obj)
// - `Serialize`   -> преобразования структуры в JSON
// - `Deserialize` -> парсинга входящего JSON или GET query-параметров
// ============================================================================

/// Входящие параметры для поиска: GET /api/search?q=Miyagi
#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub q: String,
}

/// Входящие параметры для стриминга аудио: GET /api/stream?url=...&ss=0
#[derive(Debug, Deserialize)]
pub struct StreamParams {
    /// Прямая ссылка на трек (SoundCloud / YouTube / прямая ссылка)
    pub url: Option<String>,
    /// Идентификатор трека (если передали id вместо url)
    pub id: Option<String>,
    /// Секунда, с которой начать воспроизведение (для перемотки)
    pub ss: Option<u64>,
}

/// Модель одного найденного трека для фронтенда
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchTrack {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub duration: f64,
    pub audio_url: String,
    pub cover_url: Option<String>,
}

// ============================================================================
// ВСПОМОГАТЕЛЬНЫЕ ФУНКЦИИ
// ============================================================================

/// Определяем путь к утилите yt-dlp.
/// Сначала проверяем свежую версию в ~/.local/bin/yt-dlp, иначе берем системную.
fn get_yt_dlp_cmd() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let local_path = format!("{}/.local/bin/yt-dlp", home);
    if Path::new(&local_path).exists() {
        local_path
    } else {
        "yt-dlp".to_string()
    }
}

// ============================================================================
// ОБРАБОТЧИКИ МАРШРУТОВ (Route Handlers)
//
// В Axum обработчик — это просто async функция.
// Параметры вроде `Query(params)` — это "экстракторы" (extractors). Axum
// автоматически достает нужные данные из HTTP-запроса и передает в функцию.
// ============================================================================

/// 1. Health-check: GET /api/health
/// Используется фронтендом, чтобы показать статус подключения «RUST ENGINE».
async fn health_check() -> &'static str {
    "SIGNAL // Rust Engine Online"
}

/// 2. Поиск музыки в глобальной сети: GET /api/search?q=Linkin+Park
///
/// Как это работает:
/// 1) Запускаем `yt-dlp` с запросом поиска `scsearch10:{q}` в асинхронном дочернем процессе.
/// 2) yt-dlp возвращает построчный JSON для каждого найденного трека.
/// 3) Мы построчно читаем вывод без блокировки основного потока сервера (Tokio).
/// 4) Формируем красивый JSON-список с названиями, авторами, HD обложками и ссылкой на стриминг.
async fn search_music(Query(params): Query<SearchParams>) -> Result<Json<Vec<SearchTrack>>, StatusCode> {
    let query = params.q.trim();
    if query.is_empty() {
        return Ok(Json(Vec::new()));
    }

    println!("[SEARCH] Поисковый запрос: \"{}\"", query);

    let yt_cmd = get_yt_dlp_cmd();

    // Запускаем yt-dlp. Аргумент "scsearch10:{q}" ищет 10 самых релевантных треков.
    // Флаг --flat-playlist позволяет получить метаданные моментально без скачивания.
    let mut child = Command::new(&yt_cmd)
        .args([
            &format!("scsearch10:{}", query),
            "--dump-json",
            "--flat-playlist",
            "--no-warnings",
            "--no-check-certificates",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| {
            eprintln!("[ERROR] Не удалось запустить yt-dlp: {}", err);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    // Достаем поток стандартного вывода (stdout) процесса
    let stdout = child.stdout.take().ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut reader = tokio::io::BufReader::new(stdout).lines();
    let mut tracks = Vec::new();

    // Читаем поток построчно в асинхронном цикле
    while let Ok(Some(line)) = reader.next_line().await {
        if let Ok(item) = serde_json::from_str::<serde_json::Value>(&line) {
            let id = item["id"].as_str().unwrap_or("").to_string();
            let title = item["title"].as_str().unwrap_or("Без названия").to_string();
            let artist = item["uploader"].as_str().unwrap_or("Неизвестный исполнитель").to_string();
            let duration = item["duration"].as_f64().unwrap_or(0.0);

            // Получаем ссылку на исходный трек (webpage_url или url)
            let track_url = item["webpage_url"]
                .as_str()
                .or_else(|| item["url"].as_str())
                .unwrap_or("")
                .to_string();

            // Извлекаем обложку и преобразуем к HD-разрешению (t500x500 вместо mini)
            let mut cover_url = item["thumbnails"]
                .as_array()
                .and_then(|thumbs| thumbs.first())
                .and_then(|t| t["url"].as_str())
                .map(|s| s.to_string());

            if let Some(ref c) = cover_url {
                if c.contains("-mini.") {
                    cover_url = Some(c.replace("-mini.", "-t500x500."));
                }
            }

            if !id.is_empty() {
                // Кодируем ссылку на стрим через наш локальный Rust-шлюз
                let audio_url = if !track_url.is_empty() {
                    let encoded_url = urlencoding::encode(&track_url);
                    format!("http://localhost:8085/api/stream?url={}", encoded_url)
                } else {
                    format!("http://localhost:8085/api/stream?id={}", id)
                };

                tracks.push(SearchTrack {
                    id,
                    title,
                    artist,
                    duration,
                    audio_url,
                    cover_url,
                });
            }
        }
    }

    // Дожидаемся завершения процесса, чтобы не плодить зомби-процессы
    let _ = child.wait().await;
    println!("[SEARCH] Успешно найдено треков: {}", tracks.len());

    Ok(Json(tracks))
}

/// 3. Онлайн-стриминг аудио: GET /api/stream?url=...
///
/// Сердце проекта — потоковая передача звука в реальном времени!
///
/// Как работает:
/// 1) Принимает ссылку на трек из поиска.
/// 2) Через `yt-dlp -g` извлекает прямой CDN-URL аудиопотока.
/// 3) Запускает `ffmpeg`, который на лету декодирует звук в чистый MP3 поток (192 kbps)
///    и направляет его прямо в стандартный вывод (pipe:1).
/// 4) Tokio утилита `ReaderStream` превращает stdout дочернего процесса в асинхронный HTTP Body.
/// 5) Браузер начинает играть трек сразу же с 0-й секунды без ожидания полного скачивания файла!
async fn stream_audio(
    Query(params): Query<StreamParams>,
    _client_headers: HeaderMap,
) -> Result<Response, StatusCode> {
    // Определяем цель для стриминга (приоритет у url, иначе id)
    let target = match (params.url, params.id) {
        (Some(u), _) if !u.trim().is_empty() => u.trim().to_string(),
        (_, Some(id)) if !id.trim().is_empty() => {
            if id.starts_with("http") {
                id.trim().to_string()
            } else {
                format!("https://api.soundcloud.com/tracks/{}", id.trim())
            }
        }
        _ => return Err(StatusCode::BAD_REQUEST),
    };

    println!("[STREAM] Запрос на стриминг источника: {}", target);

    let yt_cmd = get_yt_dlp_cmd();

    // 1) Извлекаем прямой URL аудиопотока через yt-dlp -g
    let output = Command::new(&yt_cmd)
        .args([
            "-g",
            "-f",
            "ba/b",
            "--no-warnings",
            "--no-check-certificates",
            &target,
        ])
        .output()
        .await
        .map_err(|e| {
            eprintln!("[ERROR] Ошибка запуска yt-dlp -g: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    if !output.status.success() {
        eprintln!("[ERROR] yt-dlp не смог получить ссылку на аудиопоток");
        return Err(StatusCode::NOT_FOUND);
    }

    let direct_url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if direct_url.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    // 2) Формируем аргументы для FFmpeg для реалтайм кодирования в чистый MP3 поток
    let mut ffmpeg_args = vec![
        "-reconnect".to_string(),
        "1".to_string(),
        "-reconnect_streamed".to_string(),
        "1".to_string(),
        "-reconnect_delay_max".to_string(),
        "5".to_string(),
    ];

    // Если запрошена перемотка на определенную секунду
    if let Some(seek_sec) = params.ss {
        if seek_sec > 0 {
            ffmpeg_args.push("-ss".to_string());
            ffmpeg_args.push(seek_sec.to_string());
        }
    }

    ffmpeg_args.extend([
        "-i".to_string(),
        direct_url,
        "-vn".to_string(),
        "-f".to_string(),
        "mp3".to_string(),
        "-b:a".to_string(),
        "192k".to_string(),
        "pipe:1".to_string(),
    ]);

    // 3) Запускаем FFmpeg как дочерний асинхронный процесс
    let mut ffmpeg_child = Command::new("ffmpeg")
        .args(&ffmpeg_args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            eprintln!("[ERROR] Не удалось запустить FFmpeg: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let stdout = ffmpeg_child
        .stdout
        .take()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    // 4) Преобразуем AsyncRead (stdout) в асинхронный стрим байтов для тела ответа HTTP
    let stream = ReaderStream::new(stdout);
    let body = Body::from_stream(stream);

    // 5) Настраиваем заголовки: отдаем как универсальный поток audio/mpeg
    let mut res_headers = HeaderMap::new();
    res_headers.insert(header::CONTENT_TYPE, "audio/mpeg".parse().unwrap());
    res_headers.insert(header::CACHE_CONTROL, "no-cache, no-store".parse().unwrap());
    res_headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*".parse().unwrap());
    res_headers.insert(header::ACCESS_CONTROL_EXPOSE_HEADERS, "*".parse().unwrap());

    println!("[STREAM] Аудиопоток успешно открыт и передается в браузер");

    Ok((StatusCode::OK, res_headers, body).into_response())
}

// ============================================================================
// ТОЧКА ВХОДА (Main Function)
//
// `#[tokio::main]` оборачивает функцию main в асинхронную среду выполнения Tokio.
// Это позволяет использовать `async / await` прямо в точке входа.
// ============================================================================

#[tokio::main]
async fn main() {
    println!("--------------------------------------------------");
    println!("  SIGNAL // Minimalist Audio Backend (Rust Axum)  ");
    println!("--------------------------------------------------");

    // Разрешаем Cross-Origin Resource Sharing (CORS) для любых клиентов
    let cors = CorsLayer::permissive();

    // Создаем роутер приложения Axum и подключаем хэндлеры
    let app = Router::new()
        .route("/api/health", get(health_check))
        .route("/api/search", get(search_music))
        .route("/api/stream", get(stream_audio))
        .layer(cors);

    // Назначаем адрес и порт: слушаем на всех интерфейсах (0.0.0.0:8085)
    let addr = SocketAddr::from(([0, 0, 0, 0], 8085));
    println!("[ONLINE] Сервер запущен: http://localhost:8085");
    println!("[ROUTES] GET /api/health     -> Проверка доступности");
    println!("[ROUTES] GET /api/search?q=..-> Поиск музыки в реальном времени");
    println!("[ROUTES] GET /api/stream?url=-> Прямой стриминг аудиопотока");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Не удалось занять порт 8085. Проверьте, не занят ли порт другим процессом.");

    // Запускаем HTTP-сервер
    axum::serve(listener, app)
        .await
        .expect("Критическая ошибка работы сервера Axum");
}
