const CACHE_NAME = 'signal-pwa-v10';
const OFFLINE_AUDIO_CACHE = 'signal-offline-tracks-v1';

const ASSETS_TO_CACHE = [
  '/',
  '/index.html',
  '/manifest.webmanifest',
  '/favicon.ico',
  '/icons/icon-192.png',
  '/icons/icon-512.png',
  '/icons/icon-maskable-512.png',
  '/icons/apple-touch-icon.png'
];

self.addEventListener('install', (event) => {
  self.skipWaiting();
  event.waitUntil(
    caches.open(CACHE_NAME).then((cache) => cache.addAll(ASSETS_TO_CACHE)).catch(() => {})
  );
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys().then((keys) =>
      Promise.all(
        keys
          .filter((key) => key !== CACHE_NAME && key !== OFFLINE_AUDIO_CACHE)
          .map((key) => {
            console.log('[SW] Purging old cache:', key);
            return caches.delete(key);
          })
      )
    ).then(() => self.clients.claim())
  );
});

self.addEventListener('fetch', (event) => {
  const url = new URL(event.request.url);

  // Do not intercept non-GET requests or backend API requests
  if (event.request.method !== 'GET' || url.pathname.startsWith('/api/')) {
    return;
  }

  // 1. Handle offline cached audio requests
  if (url.pathname.startsWith('/offline-audio/')) {
    event.respondWith(
      caches.open(OFFLINE_AUDIO_CACHE).then((cache) => {
        return cache.match(event.request.url).then((cached) => {
          return cached || fetch(event.request);
        });
      })
    );
    return;
  }

  // 2. Do not cache live audio streams in the main PWA cache
  if (
    url.pathname.includes('/api/stream') ||
    url.pathname.endsWith('.mp3') ||
    url.pathname.endsWith('.aac') ||
    url.pathname.endsWith('.m4a') ||
    url.pathname.endsWith('.opus')
  ) {
    return;
  }

  // 3. For Navigation / HTML: ALWAYS NETWORK-FIRST so updates appear instantly!
  if (
    event.request.mode === 'navigate' ||
    url.pathname === '/' ||
    url.pathname === '/index.html'
  ) {
    event.respondWith(
      fetch(event.request)
        .then((networkResponse) => {
          if (networkResponse && networkResponse.status === 200) {
            const clone = networkResponse.clone();
            caches.open(CACHE_NAME).then((cache) => cache.put(event.request, clone));
          }
          return networkResponse;
        })
        .catch(() => {
          return caches.match('/index.html').then((cached) => cached || caches.match('/'));
        })
    );
    return;
  }

  // 4. For static assets (JS, CSS, Icons): Network-First, fallback to cache
  if (url.origin === self.location.origin) {
    event.respondWith(
      fetch(event.request)
        .then((networkResponse) => {
          if (networkResponse && networkResponse.status === 200) {
            const clone = networkResponse.clone();
            caches.open(CACHE_NAME).then((cache) => cache.put(event.request, clone));
          }
          return networkResponse;
        })
        .catch(() => caches.match(event.request))
    );
  }
});
