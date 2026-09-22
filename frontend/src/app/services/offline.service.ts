import { Injectable, signal, inject, Injector } from '@angular/core';
import { Track } from '../models/track.model';
import { LibraryService } from './library.service';

const IDB_NAME = 'signal_offline_db';
const IDB_STORE = 'audio_blobs';

function openOfflineDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    if (typeof window === 'undefined' || !('indexedDB' in window)) {
      reject(new Error('IndexedDB not supported'));
      return;
    }
    const req = indexedDB.open(IDB_NAME, 1);
    req.onupgradeneeded = () => {
      const db = req.result;
      if (!db.objectStoreNames.contains(IDB_STORE)) {
        db.createObjectStore(IDB_STORE);
      }
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function idbPutBlob(id: string, blob: Blob): Promise<void> {
  const db = await openOfflineDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(IDB_STORE, 'readwrite');
    const store = tx.objectStore(IDB_STORE);
    const req = store.put(blob, id);
    req.onsuccess = () => resolve();
    req.onerror = () => reject(req.error);
  });
}

async function idbGetBlob(id: string): Promise<Blob | null> {
  try {
    const db = await openOfflineDb();
    return new Promise((resolve, reject) => {
      const tx = db.transaction(IDB_STORE, 'readonly');
      const store = tx.objectStore(IDB_STORE);
      const req = store.get(id);
      req.onsuccess = () => resolve(req.result || null);
      req.onerror = () => reject(req.error);
    });
  } catch {
    return null;
  }
}

async function idbDeleteBlob(id: string): Promise<void> {
  try {
    const db = await openOfflineDb();
    return new Promise((resolve, reject) => {
      const tx = db.transaction(IDB_STORE, 'readwrite');
      const store = tx.objectStore(IDB_STORE);
      const req = store.delete(id);
      req.onsuccess = () => resolve();
      req.onerror = () => reject(req.error);
    });
  } catch {}
}

@Injectable({
  providedIn: 'root',
})
export class OfflineService {
  private readonly CACHE_NAME = 'signal-offline-tracks-v1';
  private readonly STORAGE_KEY_OFFLINE = 'signal_offline_tracks_meta';

  private injector = inject(Injector);
  readonly offlineTrackIds = signal<Set<string>>(new Set());
  readonly downloadingTrackIds = signal<Set<string>>(new Set());

  private activeBlobUrl: string | null = null;

  constructor() {
    this.loadOfflineIndex();
  }

  private getBackendBaseUrl(): string {
    try {
      const lib = this.injector.get(LibraryService);
      return lib.getBackendUrl();
    } catch {
      return typeof window !== 'undefined' ? window.location.origin : '';
    }
  }

  private loadOfflineIndex() {
    if (typeof window === 'undefined') return;
    try {
      const raw = localStorage.getItem(this.STORAGE_KEY_OFFLINE);
      if (raw) {
        const list: Track[] = JSON.parse(raw);
        const ids = new Set(list.map((t) => t.id));
        this.offlineTrackIds.set(ids);
      }
    } catch {}
  }

  isTrackOffline(trackId: string): boolean {
    return this.offlineTrackIds().has(trackId);
  }

  isDownloading(trackId: string): boolean {
    return this.downloadingTrackIds().has(trackId);
  }

  getOfflineTracks(): Track[] {
    if (typeof window === 'undefined') return [];
    try {
      const raw = localStorage.getItem(this.STORAGE_KEY_OFFLINE);
      return raw ? JSON.parse(raw) : [];
    } catch {
      return [];
    }
  }

  /**
   * Сохраняет готовый Blob (например, загруженный локальный файл) в постоянный кэш офлайн.
   */
  async saveBlobOffline(track: Track, blob: Blob): Promise<boolean> {
    try {
      // 1. Сохраняем в IndexedDB (надежно на мобильных)
      try {
        await idbPutBlob(track.id, blob);
      } catch (e) {
        console.warn('[OfflineService] IndexedDB save failed, fallback to Cache API:', e);
      }

      // 2. Также сохраняем в Cache API
      if (typeof window !== 'undefined' && 'caches' in window) {
        try {
          const cache = await caches.open(this.CACHE_NAME);
          const cacheKey = `/offline-audio/${track.id}`;
          const responseToCache = new Response(blob, {
            status: 200,
            headers: {
              'Content-Type': blob.type || 'audio/mpeg',
              'Content-Length': blob.size.toString(),
            },
          });
          await cache.put(cacheKey, responseToCache);
        } catch {}
      }

      const existing = this.getOfflineTracks().filter((t) => t.id !== track.id);
      const updatedTrack: Track = { ...track, isOffline: true };
      existing.push(updatedTrack);
      localStorage.setItem(this.STORAGE_KEY_OFFLINE, JSON.stringify(existing));

      const updatedIds = new Set(this.offlineTrackIds());
      updatedIds.add(track.id);
      this.offlineTrackIds.set(updatedIds);
      return true;
    } catch (err) {
      console.error('[OfflineService] Failed to cache blob track:', err);
      return false;
    }
  }

  async saveTrackOffline(track: Track): Promise<boolean> {
    if (typeof window === 'undefined') {
      return false;
    }
    if (track.isLiveStream) {
      return false;
    }

    const currentDownloading = new Set(this.downloadingTrackIds());
    currentDownloading.add(track.id);
    this.downloadingTrackIds.set(currentDownloading);

    try {
      let audioUrl = track.audioUrl;
      const backendBase = this.getBackendBaseUrl();

      // Resolve relative and absolute URLs to the active backend URL
      if (!audioUrl.startsWith('blob:')) {
        if (audioUrl.startsWith('/api/stream')) {
          audioUrl = `${backendBase}${audioUrl}`;
        } else if (audioUrl.includes('/api/stream')) {
          const streamIdx = audioUrl.indexOf('/api/stream');
          audioUrl = `${backendBase}${audioUrl.slice(streamIdx)}`;
        } else if (
          audioUrl.includes('youtube.com') ||
          audioUrl.includes('youtu.be') ||
          audioUrl.includes('soundcloud.com')
        ) {
          audioUrl = `${backendBase}/api/stream?url=${encodeURIComponent(audioUrl)}`;
        } else if (!audioUrl.startsWith('http')) {
          audioUrl = `${backendBase}${audioUrl.startsWith('/') ? '' : '/'}${audioUrl}`;
        }

        // Add title/artist metadata to stream URL for better backend resolution
        if (audioUrl.includes('/api/stream')) {
          if (!audioUrl.includes('title=') && track.title) {
            const glue = audioUrl.includes('?') ? '&' : '?';
            audioUrl = `${audioUrl}${glue}title=${encodeURIComponent(track.title || '')}`;
          }
          if (!audioUrl.includes('artist=') && track.artist) {
            const glue = audioUrl.includes('?') ? '&' : '?';
            audioUrl = `${audioUrl}${glue}artist=${encodeURIComponent(track.artist || '')}`;
          }
        }
      }

      const resp = await fetch(audioUrl, { mode: 'cors' });
      if (!resp.ok) {
        throw new Error(`Failed to download audio: ${resp.status}`);
      }

      const blob = await resp.blob();

      // 1. Сохраняем в IndexedDB (гарантирует воспроизведение на iOS Safari и Android)
      try {
        await idbPutBlob(track.id, blob);
      } catch (e) {
        console.warn('[OfflineService] IndexedDB save error:', e);
      }

      // 2. Сохраняем в Cache API
      if ('caches' in window) {
        try {
          const cache = await caches.open(this.CACHE_NAME);
          const cacheKey = `/offline-audio/${track.id}`;
          const responseToCache = new Response(blob, {
            status: 200,
            headers: {
              'Content-Type': 'audio/mpeg',
              'Content-Length': blob.size.toString(),
            },
          });
          await cache.put(cacheKey, responseToCache);
        } catch {}
      }

      // 3. Сохраняем метаданные в local storage
      const existing = this.getOfflineTracks().filter((t) => t.id !== track.id);
      const updatedTrack: Track = { ...track, isOffline: true };
      existing.push(updatedTrack);
      localStorage.setItem(this.STORAGE_KEY_OFFLINE, JSON.stringify(existing));

      const updatedIds = new Set(this.offlineTrackIds());
      updatedIds.add(track.id);
      this.offlineTrackIds.set(updatedIds);

      return true;
    } catch (err) {
      console.error('[OfflineService] Failed to cache track:', err);
      return false;
    } finally {
      const dl = new Set(this.downloadingTrackIds());
      dl.delete(track.id);
      this.downloadingTrackIds.set(dl);
    }
  }

  async removeTrackOffline(trackId: string): Promise<boolean> {
    try {
      await idbDeleteBlob(trackId);

      if (typeof window !== 'undefined' && 'caches' in window) {
        try {
          const cache = await caches.open(this.CACHE_NAME);
          const cacheKey = `/offline-audio/${trackId}`;
          await cache.delete(cacheKey);
        } catch {}
      }

      const existing = this.getOfflineTracks().filter((t) => t.id !== trackId);
      localStorage.setItem(this.STORAGE_KEY_OFFLINE, JSON.stringify(existing));

      const updatedIds = new Set(this.offlineTrackIds());
      updatedIds.delete(trackId);
      this.offlineTrackIds.set(updatedIds);
      return true;
    } catch (err) {
      console.error('[OfflineService] Failed to remove cached track:', err);
      return false;
    }
  }

  async getOfflineBlobUrl(trackId: string): Promise<string | null> {
    if (typeof window === 'undefined') {
      return null;
    }

    try {
      // 1. Сначала проверяем IndexedDB
      const blob = await idbGetBlob(trackId);
      if (blob) {
        if (this.activeBlobUrl) {
          URL.revokeObjectURL(this.activeBlobUrl);
        }
        this.activeBlobUrl = URL.createObjectURL(blob);
        return this.activeBlobUrl;
      }

      // 2. Фолбэк на Cache API
      if ('caches' in window) {
        const cache = await caches.open(this.CACHE_NAME);
        const cacheKey = `/offline-audio/${trackId}`;
        const match = await cache.match(cacheKey);
        if (match) {
          const b = await match.blob();
          // Мигрируем в IndexedDB для будущей скорости
          idbPutBlob(trackId, b).catch(() => {});
          if (this.activeBlobUrl) {
            URL.revokeObjectURL(this.activeBlobUrl);
          }
          this.activeBlobUrl = URL.createObjectURL(b);
          return this.activeBlobUrl;
        }
      }
    } catch (e) {
      console.warn('[OfflineService] getOfflineBlobUrl error:', e);
    }

    return null;
  }
}
