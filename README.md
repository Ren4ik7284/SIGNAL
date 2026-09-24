# Recro - Minimalist Audio Player

Recro - независимый веб-плеер для прослушивания музыки, плейлистов и онлайн-радио без рекламы и без необходимости использовать VPN. Все аудиопотоки воспроизводятся напрямую через бэкенд.

## Рабочие ссылки

- Frontend: https://signal-frontend-production-a944.up.railway.app
- Backend: https://signal-audio-backend-production.up.railway.app

## Основные возможности

- Поиск и прямое воспроизведение музыки без капч и ограничений
- Поддержка ссылок и плейлистов с YouTube с автоматическим резервным поиском
- Каталог мировых онлайн-радиостанций с поиском по жанрам и странам
- Автоматическая фоновая синхронизация медиатеки с сервером
- Адаптивный мобильный интерфейс и нижняя панель управления
- Локальный экспорт и импорт базы треков в JSON

## Быстрый запуск через Docker

Запуск всего проекта (фронтенд + бэкенд) одной командой:

```bash
docker compose up --build -d
```

- Frontend доступен по адресу: http://localhost:4200
- Backend доступен по адресу: http://localhost:8085

Остановка контейнеров:

```bash
docker compose down
```

## Ручной запуск для разработки

### 1. Бэкенд (Rust)

Требования: Rust, Cargo, FFmpeg, yt-dlp.

```bash
cd backend
cargo run
```

Сервер запускается на `http://localhost:8085`.

### 2. Фронтенд (Angular)

Требования: Node.js 20+, npm.

```bash
cd frontend
npm install
npm start
```

Интерфейс открывается в браузере по адресу `http://localhost:4200`.

## О Dockerfile

- `backend/Dockerfile`: двухэтапная сборка. На первом этапе компилируется релизный бинарник на Rust (`rust:trixie`), на втором собирается легковесный образ на базе `debian:trixie-slim` с установленными FFmpeg, Python 3, Node.js и yt-dlp.
- `frontend/Dockerfile`: сборка Angular-приложения в production (`node:20-alpine`) и раздача статики через `nginx:alpine` с маршрутизацией SPA.
