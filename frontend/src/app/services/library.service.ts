import { Injectable, signal, computed, inject, OnDestroy } from '@angular/core';
import { Track, Playlist, RadioStation } from '../models/track.model';
import { OfflineService } from './offline.service';
import { AuthService, HistoryItem, WrappedStats } from './auth.service';

export interface ExtractedResult {
  playlistTitle: string | null;
  tracks: Track[];
  mainVideo?: Track | null;
  isRadioMix?: boolean;
  hasChapters?: boolean;
}

@Injectable({
  providedIn: 'root',
})
export class LibraryService implements OnDestroy {
  private activeBackendUrl =
    typeof window !== 'undefined' && window.location.hostname !== 'localhost' && window.location.hostname !== '127.0.0.1'
      ? window.location.origin
      : 'https://signal-audio-backend-production.up.railway.app';
  private readonly FALLBACK_BACKEND_URL = 'https://signal-audio-backend-production.up.railway.app';

  readonly offlineService = inject(OfflineService);
  readonly authService = inject(AuthService);

  // --- Ключи localStorage изолированы по user_id ---
  // Гость использует 'guest', авторизованный — свой UUID.
  // Это гарантирует что два пользователя на одном браузере видят только своё.
  private get storageUserId(): string {
    return this.authService.currentUser()?.id || 'guest';
  }

  private get STORAGE_KEY_TRACKS(): string {
    return `signal_tracks_${this.storageUserId}`;
  }
  private get STORAGE_KEY_FAVORITES(): string {
    return `signal_favs_${this.storageUserId}`;
  }
  private get STORAGE_KEY_PLAYLISTS(): string {
    return `signal_playlists_${this.storageUserId}`;
  }
  private get STORAGE_KEY_STATIONS(): string {
    return `signal_stations_${this.storageUserId}`;
  }
  private get STORAGE_KEY_UPDATED_AT(): string {
    return `signal_updated_at_${this.storageUserId}`;
  }

  readonly defaultTracks: Track[] = [
    {
      id: 'default-track-1',
      title: 'red weather',
      artist: 'ONDA ANDAR',
      album: 'Online Music',
      duration: 104,
      audioUrl: '/api/stream?url=https%3A%2F%2Fwww.youtube.com%2Fwatch%3Fv%3DSUCefsDmjk0&title=NO%20ESPERABA%20ESTO&artist=ONDA%20ANDAR',
      coverUrl: '/api/cover?url=https%3A%2F%2Fi.ytimg.com%2Fvi%2FSUCefsDmjk0%2Fhq720.jpg',
      genre: 'Electronic',
      format: 'mp3',
      bitrate: '192 kbps',
      plays: 0,
      isFavorite: false,
      addedAt: '2026-09-11',
    },
    {
      id: 'default-track-2',
      title: 'Недоволен',
      artist: 'Scally Milano',
      album: 'Online Music',
      duration: 121,
      audioUrl: '/api/stream?url=https%3A%2F%2Fwww.youtube.com%2Fwatch%3Fv%3DHVXlDpNainw&title=%D0%9D%D0%B5%D0%B4%D0%BE%D0%B2%D0%BE%D0%BB%D0%B5%D0%BD&artist=Scally%20Milano',
      coverUrl: '/api/cover?url=https%3A%2F%2Fi.ytimg.com%2Fvi%2FHVXlDpNainw%2Fhq720.jpg',
      genre: 'Hip-Hop',
      format: 'mp3',
      bitrate: '192 kbps',
      plays: 0,
      isFavorite: false,
      addedAt: '2026-09-11',
    },
    {
      id: 'default-track-3',
      title: 'ДИНАСТИЯ',
      artist: 'wo',
      album: 'SoundCloud',
      duration: 150,
      audioUrl: '/api/stream?url=https%3A%2F%2Fsoundcloud.com%2Fwwoowwoo%2Fvillian-madk1d-dinastiia',
      coverUrl: 'https://i1.sndcdn.com/artworks-Z9NZvqkNSb4Z1xrV-E5zm9w-t500x500.jpg',
      genre: 'Electronic',
      format: 'mp3',
      bitrate: '192 kbps',
      plays: 0,
      isFavorite: true,
      addedAt: '2026-09-11',
    },
    {
      id: 'default-track-4',
      title: 'Mania (Fl Studio Session)',
      artist: 'SchuberTEKK',
      album: 'SoundCloud',
      duration: 252,
      audioUrl: '/api/stream?url=https%3A%2F%2Fsoundcloud.com%2Forlando-267593096%2Fmania',
      coverUrl: 'https://i1.sndcdn.com/artworks-VxzuqknJ4ffnSJfg-aAYIYw-t500x500.jpg',
      genre: 'Techno',
      format: 'mp3',
      bitrate: '192 kbps',
      plays: 0,
      isFavorite: false,
      addedAt: '2026-09-11',
    },
    {
      id: 'default-track-5',
      title: 'Drone Zone 24/7',
      artist: 'SomaFM Stream',
      album: 'Live Radio Stations',
      duration: 0,
      audioUrl: 'https://ice2.somafm.com/dronezone-128-mp3',
      genre: 'Ambient',
      format: 'stream',
      bitrate: '128k Live',
      plays: 0,
      isFavorite: false,
      addedAt: '2026-09-11',
      isLiveStream: true,
    },
  ];

  readonly defaultRadioStations: RadioStation[] = [
    { id: 'default-1', name: 'Record Chill-Out', streamUrl: 'https://radiorecord.hostingradio.ru/chil96.aacp', genre: 'Chillout / Lounge', country: 'RU', bitrate: '96k AAC' },
    { id: 'default-2', name: 'Europa Plus', streamUrl: 'https://ep256.hostingradio.ru:8052/europaplus256.mp3', genre: 'Pop / Top 40', country: 'RU', bitrate: '256k MP3' },
    { id: 'default-3', name: 'SomaFM: Groove Salad', streamUrl: 'https://ice4.somafm.com/groovesalad-128-mp3', genre: 'Ambient / Downtempo', country: 'US', bitrate: '128k Live' },
    { id: 'default-4', name: 'SomaFM: Drone Zone', streamUrl: 'https://ice2.somafm.com/dronezone-128-mp3', genre: 'Space / Atmospheric', country: 'US', bitrate: '128k Live' },
    { id: 'default-5', name: 'SomaFM: DEF CON Radio', streamUrl: 'https://ice6.somafm.com/defcon-128-mp3', genre: 'Electronic / Cyber', country: 'US', bitrate: '128k Live' },
    { id: 'default-6', name: 'Record Deep', streamUrl: 'https://radiorecord.hostingradio.ru/deep96.aacp', genre: 'Deep House', country: 'RU', bitrate: '96k AAC' },
    { id: 'default-7', name: 'Record Synthwave', streamUrl: 'https://radiorecord.hostingradio.ru/synth96.aacp', genre: 'Synthwave / Retro', country: 'RU', bitrate: '96k AAC' },
    { id: 'default-8', name: 'Record Lo-Fi', streamUrl: 'https://radiorecord.hostingradio.ru/lofi96.aacp', genre: 'Lo-Fi / Beats', country: 'RU', bitrate: '96k AAC' },
    { id: 'default-9', name: 'SomaFM: Secret Agent', streamUrl: 'https://ice1.somafm.com/secretagent-128-mp3', genre: 'Spy / Lounge', country: 'US', bitrate: '128k Live' },
    { id: 'default-10', name: 'Record Russian Hits', streamUrl: 'https://radiorecord.hostingradio.ru/rus96.aacp', genre: 'Pop / Russian', country: 'RU', bitrate: '96k AAC' },
  ];

  readonly tracks = signal<Track[]>([]);
  readonly playlists = signal<Playlist[]>([]);
  readonly radioStations = signal<RadioStation[]>([]);
  readonly searchQuery = signal<string>('');
  readonly selectedGenre = signal<string>('all');
  readonly selectedView = signal<'all' | 'favorites' | 'uploads' | 'streams' | 'playlist' | 'offline'>('all');
  readonly activePlaylistId = signal<string | null>(null);

  readonly onlineSearchResults = signal<Track[]>([]);
  readonly isSearchingOnline = signal<boolean>(false);
  readonly isBackendOnline = signal<boolean>(false);

  readonly availableGenres = computed(() => {
    const all = this.tracks().map((t) => t.genre).filter(Boolean);
    return ['all', ...Array.from(new Set(all))];
  });

  readonly offlineTracksCount = computed(() => {
    return this.tracks().filter((t) => this.offlineService.isTrackOffline(t.id)).length;
  });

  readonly allTracksCount = computed(() => {
    return this.tracks().filter((t) => !t.playlistOnly).length;
  });

  readonly filteredTracks = computed(() => {
    const query = this.searchQuery().trim().toLowerCase();
    const genre = this.selectedGenre();
    const view = this.selectedView();
    const playlistId = this.activePlaylistId();

    return this.tracks().filter((track) => {
      if (view === 'all' && track.playlistOnly) return false;
      if (view === 'favorites' && !track.isFavorite) return false;
      if (view === 'uploads' && !track.isLocalUpload) return false;
      if (view === 'streams' && !track.isLiveStream && track.format !== 'stream') return false;
      if (view === 'offline' && !this.offlineService.isTrackOffline(track.id)) return false;
      if (view === 'playlist' && playlistId) {
        const pl = this.playlists().find((p) => p.id === playlistId);
        if (!pl || !pl.trackIds.includes(track.id)) return false;
      }

      if (genre !== 'all' && track.genre.toLowerCase() !== genre.toLowerCase()) {
        return false;
      }

      if (query) {
        const matchTitle = track.title.toLowerCase().includes(query);
        const matchArtist = track.artist.toLowerCase().includes(query);
        const matchAlbum = track.album?.toLowerCase().includes(query) ?? false;
        const matchGenre = track.genre.toLowerCase().includes(query);
        const matchUrl = track.audioUrl.toLowerCase().includes(query);
        if (!matchTitle && !matchArtist && !matchAlbum && !matchGenre && !matchUrl) {
          return false;
        }
      }

      return true;
    });
  });

  readonly isCloudSynced = signal<boolean>(false);
  private syncTimeout: ReturnType<typeof setTimeout> | null = null;
  private syncPollInterval: ReturnType<typeof setInterval> | null = null;
  private visibilityHandler: (() => void) | null = null;
  private focusHandler: (() => void) | null = null;

  constructor() {
    this.initLibrary();
    this.checkBackendHealth().then(() => {
      this.syncWithBackendOnStartup();
      this.startAutoSync();
    });
  }

  ngOnDestroy(): void {
    if (this.syncPollInterval) clearInterval(this.syncPollInterval);
    if (this.syncTimeout) clearTimeout(this.syncTimeout);
    if (this.visibilityHandler) document.removeEventListener('visibilitychange', this.visibilityHandler);
    if (this.focusHandler) window.removeEventListener('focus', this.focusHandler);
  }

  getBackendUrl(): string {
    const isHttps = typeof window !== 'undefined' && window.location.protocol === 'https:';
    if (isHttps && this.activeBackendUrl.startsWith('http://')) {
      return this.FALLBACK_BACKEND_URL;
    }
    return this.activeBackendUrl || this.FALLBACK_BACKEND_URL;
  }

  startAutoSync() {
    if (this.syncPollInterval || typeof window === 'undefined') return;

    this.syncPollInterval = setInterval(() => {
      this.syncWithBackendOnStartup();
    }, 5000);

    this.visibilityHandler = () => {
      if (document.visibilityState === 'visible') {
        this.syncWithBackendOnStartup();
      }
    };
    this.focusHandler = () => this.syncWithBackendOnStartup();

    document.addEventListener('visibilitychange', this.visibilityHandler);
    window.addEventListener('focus', this.focusHandler);
  }

  private initLibrary() {
    let savedTracks: Track[] = [];
    try {
      const stored = localStorage.getItem(this.STORAGE_KEY_TRACKS);
      if (stored) savedTracks = JSON.parse(stored);
    } catch {
      savedTracks = [];
    }

    let favIds: string[] = [];
    try {
      const storedFavs = localStorage.getItem(this.STORAGE_KEY_FAVORITES);
      if (storedFavs) favIds = JSON.parse(storedFavs);
    } catch {
      favIds = [];
    }

    let savedPlaylists: Playlist[] = [];
    try {
      const storedPl = localStorage.getItem(this.STORAGE_KEY_PLAYLISTS);
      if (storedPl) savedPlaylists = JSON.parse(storedPl);
    } catch {
      savedPlaylists = [];
    }

    let savedStations: RadioStation[] = [];
    try {
      const storedSt = localStorage.getItem(this.STORAGE_KEY_STATIONS);
      if (storedSt) savedStations = JSON.parse(storedSt);
    } catch {
      savedStations = [];
    }

    if (!savedTracks || savedTracks.length === 0) {
      savedTracks = [...this.defaultTracks];
      try {
        localStorage.setItem(this.STORAGE_KEY_TRACKS, JSON.stringify(savedTracks));
      } catch {}
    }

    const hasBrokenStations = savedStations.some(
      (s) => s.streamUrl.includes(':8030') || s.streamUrl.includes('wostreaming.net') || s.streamUrl.includes('stream.zeno.fm')
    );
    if (!savedStations || savedStations.length === 0 || hasBrokenStations) {
      savedStations = [...this.defaultRadioStations];
      try {
        localStorage.setItem(this.STORAGE_KEY_STATIONS, JSON.stringify(savedStations));
      } catch {}
    }

    const processedTracks = savedTracks.map((t) => ({
      ...t,
      isFavorite: favIds.includes(t.id),
    }));

    this.tracks.set(processedTracks);
    this.playlists.set(savedPlaylists);
    this.radioStations.set(savedStations);
  }

  formatCoverUrl(coverUrl?: string): string | undefined {
    if (!coverUrl) return undefined;
    if (coverUrl.startsWith('/')) {
      return `${this.getBackendUrl()}${coverUrl}`;
    }
    if (coverUrl.includes('ytimg.com')) {
      return `${this.getBackendUrl()}/api/cover?url=${encodeURIComponent(coverUrl)}`;
    }
    return coverUrl;
  }

  async checkBackendHealth() {
    const isHttps = typeof window !== 'undefined' && window.location.protocol === 'https:';
    const hostname = typeof window !== 'undefined' ? window.location.hostname : 'localhost';
    const isRemoteHost = hostname !== 'localhost' && hostname !== '127.0.0.1';
    const urlsToTest: string[] = [];

    if (isRemoteHost && typeof window !== 'undefined') {
      urlsToTest.push(window.location.origin);
      urlsToTest.push(this.FALLBACK_BACKEND_URL);
    } else if (isHttps) {
      urlsToTest.push(this.FALLBACK_BACKEND_URL);
    } else {
      urlsToTest.push('http://localhost:8085');
      urlsToTest.push(this.FALLBACK_BACKEND_URL);
    }

    for (const testUrl of urlsToTest) {
      try {
        const res = await fetch(`${testUrl}/api/health`, { signal: AbortSignal.timeout(3000) });
        if (res.ok) {
          this.activeBackendUrl = testUrl;
          this.isBackendOnline.set(true);
          this.authService.verifyRemoteSession(this.activeBackendUrl);
          return;
        }
      } catch {}
    }

    this.activeBackendUrl = this.FALLBACK_BACKEND_URL;
    this.isBackendOnline.set(true);
    this.authService.verifyRemoteSession(this.activeBackendUrl);
  }

  async searchOnline(query: string): Promise<Track[]> {
    const q = query.trim();
    if (!q) {
      this.onlineSearchResults.set([]);
      return [];
    }

    this.isSearchingOnline.set(true);
    try {
      const res = await fetch(`${this.getBackendUrl()}/api/search?q=${encodeURIComponent(q)}`);
      if (!res.ok) throw new Error('Search failed');

      const data: { id: string; title: string; artist: string; duration: number; audio_url: string; cover_url?: string }[] = await res.json();

      const tracks: Track[] = data.map((item) => ({
        id: 'yt-' + item.id,
        title: item.title,
        artist: item.artist,
        duration: Math.round(item.duration),
        audioUrl: item.audio_url,
        coverUrl: this.formatCoverUrl(item.cover_url),
        genre: 'YouTube',
        format: 'mp3',
        bitrate: '192 kbps',
        plays: 0,
        isFavorite: false,
        addedAt: new Date().toISOString().split('T')[0],
      }));

      this.onlineSearchResults.set(tracks);
      return tracks;
    } catch (e) {
      console.warn('Online search error:', e);
      return [];
    } finally {
      this.isSearchingOnline.set(false);
    }
  }

  async extractFromUrl(url: string): Promise<ExtractedResult> {
    const targetUrl = url.trim();
    if (!targetUrl) return { playlistTitle: null, tracks: [] };

    const res = await fetch(`${this.getBackendUrl()}/api/extract?url=${encodeURIComponent(targetUrl)}`);
    if (!res.ok) throw new Error('Extract failed');

    const data: {
      playlist_title: string | null;
      tracks: { id: string; title: string; artist: string; duration: number; audio_url: string; cover_url?: string }[];
      main_video?: { id: string; title: string; artist: string; duration: number; audio_url: string; cover_url?: string } | null;
      is_radio_mix?: boolean;
      has_chapters?: boolean;
    } = await res.json();

    const seenIds = new Set<string>();
    const tracks: Track[] = [];

    for (const item of data.tracks || []) {
      const cleanId = 'yt-' + item.id;
      if (seenIds.has(cleanId)) continue;
      seenIds.add(cleanId);

      tracks.push({
        id: cleanId,
        title: item.title,
        artist: item.artist,
        duration: Math.round(item.duration),
        audioUrl: item.audio_url,
        coverUrl: this.formatCoverUrl(item.cover_url),
        genre: 'YouTube',
        format: 'mp3',
        bitrate: '192 kbps',
        plays: 0,
        isFavorite: false,
        addedAt: new Date().toISOString().split('T')[0],
      });
    }

    let mainVideo: Track | null = null;
    if (data.main_video) {
      mainVideo = {
        id: 'yt-' + data.main_video.id,
        title: data.main_video.title,
        artist: data.main_video.artist,
        duration: Math.round(data.main_video.duration),
        audioUrl: data.main_video.audio_url,
        coverUrl: this.formatCoverUrl(data.main_video.cover_url),
        genre: 'YouTube Mix',
        format: 'mp3',
        bitrate: '192 kbps',
        plays: 0,
        isFavorite: false,
        addedAt: new Date().toISOString().split('T')[0],
      };
    }

    return {
      playlistTitle: data.playlist_title,
      tracks,
      mainVideo,
      isRadioMix: !!data.is_radio_mix,
      hasChapters: !!data.has_chapters,
    };
  }

  importPlaylist(title: string, tracks: Track[]): Playlist {
    const newTracks: Track[] = [];
    const finalTrackIds: string[] = [];

    for (const t of tracks) {
      const existing = this.tracks().find((x) => x.id === t.id || x.audioUrl === t.audioUrl);
      if (existing) {
        finalTrackIds.push(existing.id);
      } else {
        const playlistTrack: Track = { ...t, playlistOnly: true };
        newTracks.push(playlistTrack);
        finalTrackIds.push(playlistTrack.id);
      }
    }

    if (newTracks.length > 0) {
      this.tracks.update((current) => [...newTracks, ...current]);
      this.persistTracks();
    }

    const playlist = this.createPlaylist(title, `Импортировано: ${finalTrackIds.length} треков`);

    this.playlists.update((pls) =>
      pls.map((p) => (p.id === playlist.id ? { ...p, trackIds: finalTrackIds } : p))
    );
    this.persistPlaylists();

    return playlist;
  }

  private getLocalUpdatedAt(): number {
    try {
      const v = localStorage.getItem(this.STORAGE_KEY_UPDATED_AT);
      return v ? parseInt(v, 10) || 0 : 0;
    } catch {
      return 0;
    }
  }

  private markUpdated() {
    try {
      localStorage.setItem(this.STORAGE_KEY_UPDATED_AT, Date.now().toString());
    } catch {}
    this.scheduleCloudSync();
  }

  private persistTracks() {
    const persistable = this.tracks().filter((t) => !t.audioUrl.startsWith('blob:'));
    try {
      localStorage.setItem(this.STORAGE_KEY_TRACKS, JSON.stringify(persistable));
    } catch (e) {
      console.warn('Failed to save tracks to localStorage:', e);
    }
    this.markUpdated();
  }

  private persistPlaylists() {
    try {
      localStorage.setItem(this.STORAGE_KEY_PLAYLISTS, JSON.stringify(this.playlists()));
    } catch (e) {
      console.warn('Failed to save playlists to localStorage:', e);
    }
    this.markUpdated();
  }

  private persistStations() {
    try {
      localStorage.setItem(this.STORAGE_KEY_STATIONS, JSON.stringify(this.radioStations()));
    } catch (e) {
      console.warn('Failed to save stations to localStorage:', e);
    }
    this.markUpdated();
  }

  private saveLocalWithoutCloudSync() {
    const persistable = this.tracks().filter((t) => !t.audioUrl.startsWith('blob:'));
    try {
      localStorage.setItem(this.STORAGE_KEY_TRACKS, JSON.stringify(persistable));
      localStorage.setItem(this.STORAGE_KEY_PLAYLISTS, JSON.stringify(this.playlists()));
      localStorage.setItem(this.STORAGE_KEY_STATIONS, JSON.stringify(this.radioStations()));
      const favs = persistable.filter((t) => t.isFavorite).map((t) => t.id);
      localStorage.setItem(this.STORAGE_KEY_FAVORITES, JSON.stringify(favs));
    } catch (e) {
      console.warn('Local save error:', e);
    }
  }

  scheduleCloudSync() {
    this.isCloudSynced.set(false);
    if (this.syncTimeout) clearTimeout(this.syncTimeout);
    this.syncTimeout = setTimeout(() => {
      this.pushLibraryToBackend();
    }, 1500);
  }

  async pushLibraryToBackend() {
    if (!this.isBackendOnline()) return;
    // Синкаем только авторизованных — у каждого своя библиотека на сервере
    if (!this.authService.isAuthenticated()) return;

    try {
      const updatedAt = Date.now();
      try {
        localStorage.setItem(this.STORAGE_KEY_UPDATED_AT, updatedAt.toString());
      } catch {}

      const payload = {
        updated_at: updatedAt,
        tracks: this.tracks().filter((t) => !t.audioUrl.startsWith('blob:')),
        playlists: this.playlists(),
        radio_stations: this.radioStations(),
      };

      const res = await fetch(`${this.getBackendUrl()}/api/sync`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          ...this.authService.getAuthHeaders(),
        },
        body: JSON.stringify(payload),
      });

      if (res.ok) {
        this.isCloudSynced.set(true);
      }
    } catch (e) {
      console.warn('Background sync failed:', e);
    }
  }

  /**
   * Синхронизация с бэкендом.
   * Умное слияние — НЕ стирает локальные треки, а ОБЪЕДИНЯЕТ.
   *
   * Логика:
   * 1. Нет авторизации → не трогаем ничего (локальные данные гостя)
   * 2. Только дефолтные треки → берём из облака полностью (первый вход)
   * 3. Есть свои треки → объединяем (union по id и audioUrl), не заменяем
   * 4. Локальное новее → пушим в облако
   */
  async syncWithBackendOnStartup(forceCloud = false) {
    if (!this.isBackendOnline()) return;
    // Без авторизации не синкаемся — у каждого своя история
    if (!this.authService.isAuthenticated()) return;

    try {
      const res = await fetch(`${this.getBackendUrl()}/api/sync`, {
        headers: this.authService.getAuthHeaders(),
      });
      if (!res.ok) return;
      const data = await res.json();
      if (!data) return;

      const cloudUpdatedAt = typeof data.updated_at === 'number' ? data.updated_at : 0;
      const localUpdatedAt = this.getLocalUpdatedAt();
      const localTracks = this.tracks();
      const isOnlyDefaultTracks =
        localTracks.length === 0 ||
        localTracks.every((t) => t.id.startsWith('default-track-'));

      // Случай 1: первый вход или force → берём из облака целиком
      if (
        Array.isArray(data.tracks) &&
        data.tracks.length > 0 &&
        (forceCloud || isOnlyDefaultTracks)
      ) {
        this.tracks.set(data.tracks);
        if (Array.isArray(data.playlists)) this.playlists.set(data.playlists);
        if (Array.isArray(data.radio_stations) && data.radio_stations.length > 0) {
          this.radioStations.set(data.radio_stations);
        }
        this.saveLocalWithoutCloudSync();
        try {
          localStorage.setItem(this.STORAGE_KEY_UPDATED_AT, (cloudUpdatedAt || Date.now()).toString());
        } catch {}
        this.isCloudSynced.set(true);
        return;
      }

      // Случай 2: умное двустороннее слияние
      // Добавляем треки из облака которых нет локально (по id И по audioUrl)
      if (Array.isArray(data.tracks) && data.tracks.length > 0) {
        const currentTracks = this.tracks();
        const localIds = new Set(currentTracks.map((t) => t.id));
        const localUrls = new Set(currentTracks.map((t) => t.audioUrl));
        const missingFromLocal = (data.tracks as Track[]).filter(
          (t) => !localIds.has(t.id) && !localUrls.has(t.audioUrl)
        );
        if (missingFromLocal.length > 0) {
          this.tracks.update((cur) => [...missingFromLocal, ...cur]);
          this.saveLocalWithoutCloudSync();
        }
      }

      // Добавляем плейлисты из облака которых нет локально
      if (Array.isArray(data.playlists) && data.playlists.length > 0) {
        const localPlIds = new Set(this.playlists().map((p) => p.id));
        const missingPls = (data.playlists as Playlist[]).filter((p) => !localPlIds.has(p.id));
        if (missingPls.length > 0) {
          this.playlists.update((cur) => [...cur, ...missingPls]);
        }
      }

      // Случай 3: локальное новее → пушим в облако
      if (localTracks.length > 0 && !isOnlyDefaultTracks && localUpdatedAt > cloudUpdatedAt) {
        await this.pushLibraryToBackend();
        return;
      }

      this.isCloudSynced.set(true);
    } catch (e) {
      console.warn('Sync error:', e);
    }
  }

  toggleFavorite(trackId: string) {
    this.tracks.update((current) =>
      current.map((t) => {
        if (t.id === trackId) {
          return { ...t, isFavorite: !t.isFavorite };
        }
        return t;
      })
    );
    const favs = this.tracks()
      .filter((t) => t.isFavorite)
      .map((t) => t.id);
    try {
      localStorage.setItem(this.STORAGE_KEY_FAVORITES, JSON.stringify(favs));
    } catch {}
    this.persistTracks();
  }

  addTrackToLibrary(track: Track) {
    const existing = this.tracks().find((t) => t.id === track.id || t.audioUrl === track.audioUrl);
    if (!existing) {
      this.tracks.update((cur) => [{ ...track, playlistOnly: false }, ...cur]);
      this.persistTracks();
      this.pushLibraryToBackend();
    } else if (existing.playlistOnly) {
      this.tracks.update((cur) =>
        cur.map((t) => (t.id === existing.id ? { ...t, playlistOnly: false } : t))
      );
      this.persistTracks();
      this.pushLibraryToBackend();
    }
  }

  addMultipleTracks(tracks: Track[]) {
    const toAdd: Track[] = [];
    const idsToUnmarkPlaylistOnly = new Set<string>();

    for (const t of tracks) {
      const existing = this.tracks().find((x) => x.id === t.id || x.audioUrl === t.audioUrl);
      if (!existing) {
        toAdd.push({ ...t, playlistOnly: false });
      } else if (existing.playlistOnly) {
        idsToUnmarkPlaylistOnly.add(existing.id);
      }
    }

    if (toAdd.length > 0 || idsToUnmarkPlaylistOnly.size > 0) {
      this.tracks.update((cur) => {
        let updated = cur;
        if (idsToUnmarkPlaylistOnly.size > 0) {
          updated = updated.map((t) => (idsToUnmarkPlaylistOnly.has(t.id) ? { ...t, playlistOnly: false } : t));
        }
        return [...toAdd, ...updated];
      });
      this.persistTracks();
      this.pushLibraryToBackend();
    }
  }

  addRadioStation(station: { name: string; streamUrl: string; genre?: string; country?: string; bitrate?: string }) {
    const cleanUrl = station.streamUrl.trim();
    if (!cleanUrl) return;

    const newStation: RadioStation = {
      id: 'custom-' + Date.now(),
      name: station.name.trim() || 'Пользовательская станция',
      streamUrl: cleanUrl,
      genre: station.genre?.trim() || 'Radio',
      country: station.country?.trim() || 'Custom',
      bitrate: station.bitrate?.trim() || 'Live',
      isCustom: true,
    };

    this.radioStations.update((cur) => [newStation, ...cur]);
    this.persistStations();
  }

  removeRadioStation(stationId: string) {
    this.radioStations.update((cur) => cur.filter((s) => s.id !== stationId));
    this.persistStations();
  }

  resetDefaultStations() {
    this.radioStations.set([...this.defaultRadioStations]);
    this.persistStations();
  }

  createTrackFromStation(station: RadioStation): Track {
    const track: Track = {
      id: 'radio-' + station.id,
      title: station.name,
      artist: station.country ? `Радио (${station.country})` : 'Интернет-радио',
      album: 'Live Radio Stations',
      duration: 0,
      audioUrl: station.streamUrl,
      coverUrl: station.favicon,
      genre: station.genre || 'Radio',
      format: 'stream',
      bitrate: station.bitrate || 'Live Stream',
      plays: 0,
      isFavorite: false,
      addedAt: new Date().toISOString().split('T')[0],
      isLiveStream: true,
    };
    this.addTrackToLibrary(track);
    return track;
  }

  async searchRadioBrowser(query: string): Promise<RadioStation[]> {
    const q = query.trim();
    if (!q) return [];

    try {
      const res = await fetch(`https://de1.api.radio-browser.info/json/stations/byname/${encodeURIComponent(q)}?limit=20`);
      if (!res.ok) throw new Error('Radio search failed');

      const data: any[] = await res.json();
      return data.map((item) => ({
        id: 'rb-' + (item.stationuuid || Math.random()),
        name: item.name,
        streamUrl: item.url_resolved || item.url,
        genre: item.tags ? item.tags.split(',').slice(0, 2).join(' / ') : 'Radio',
        country: item.countrycode || item.country || '',
        bitrate: item.bitrate ? `${item.bitrate}k` : 'Live',
        favicon: item.favicon || undefined,
        isCustom: false,
      }));
    } catch {
      return [];
    }
  }

  async addStreamTrack(
    url: string,
    customTitle?: string,
    customArtist?: string,
    customGenre?: string,
    isLive = false
  ): Promise<Track> {
    const cleanUrl = url.trim();

    let format: Track['format'] = 'stream';
    if (cleanUrl.endsWith('.flac')) format = 'flac';
    else if (cleanUrl.endsWith('.wav')) format = 'wav';
    else if (cleanUrl.endsWith('.ogg')) format = 'ogg';
    else if (cleanUrl.endsWith('.m4a')) format = 'm4a';
    else if (cleanUrl.endsWith('.mp3')) format = 'mp3';

    let title = customTitle?.trim();
    if (!title) {
      try {
        const pathname = new URL(cleanUrl).pathname;
        const lastPart = pathname.split('/').pop() || 'Web Stream';
        title = decodeURIComponent(lastPart.replace(/\.[^/.]+$/, '')) || 'Web Audio Stream';
      } catch {
        title = 'Web Audio Stream';
      }
    }

    const artist = customArtist?.trim() || (isLive ? 'Live Radio' : 'Network Stream');
    const genre = customGenre?.trim() || (isLive ? 'Radio' : 'Web Stream');

    const isYtOrSc =
      cleanUrl.includes('youtube.com') ||
      cleanUrl.includes('youtu.be') ||
      cleanUrl.includes('soundcloud.com');

    let finalAudioUrl = cleanUrl;
    if (isYtOrSc) {
      finalAudioUrl = `/api/stream?url=${encodeURIComponent(cleanUrl)}&title=${encodeURIComponent(title)}&artist=${encodeURIComponent(artist)}`;
    }

    let duration = 0;
    if (!isLive && !isYtOrSc) {
      duration = await new Promise<number>((resolve) => {
        const probeAudio = new Audio(cleanUrl);
        probeAudio.addEventListener('loadedmetadata', () => {
          const d = probeAudio.duration;
          resolve(isFinite(d) && d > 0 ? Math.round(d) : 0);
        });
        probeAudio.addEventListener('error', () => resolve(0));
        setTimeout(() => resolve(0), 2000);
      });
    }

    const newTrack: Track = {
      id: 'stream-' + Date.now() + '-' + Math.floor(Math.random() * 1000),
      title,
      artist,
      album: isLive ? 'Live Radio Stations' : 'Web Streams',
      duration: duration || 0,
      audioUrl: finalAudioUrl,
      genre,
      year: new Date().getFullYear(),
      format,
      bitrate: isLive ? 'Live Stream' : 'Direct URL',
      plays: 0,
      isFavorite: false,
      addedAt: new Date().toISOString().split('T')[0],
      isLiveStream: isLive || duration === 0,
    };

    this.tracks.update((current) => [newTrack, ...current]);
    this.persistTracks();
    this.pushLibraryToBackend();
    return newTrack;
  }

  async addUploadedFile(
    file: File,
    customTitle?: string,
    customArtist?: string,
    customGenre?: string
  ): Promise<Track> {
    const audioUrl = URL.createObjectURL(file);

    const ext = file.name.split('.').pop()?.toLowerCase() || 'mp3';
    const validFormat: Track['format'] = ['mp3', 'wav', 'flac', 'ogg', 'm4a'].includes(ext)
      ? (ext as Track['format'])
      : 'mp3';

    const rawName = file.name.replace(/\.[^/.]+$/, '');
    let title = customTitle || rawName;
    let artist = customArtist || 'Локальный файл';

    if (!customTitle && rawName.includes(' - ')) {
      const parts = rawName.split(' - ');
      artist = parts[0].trim();
      title = parts.slice(1).join(' - ').trim();
    }

    const duration = await new Promise<number>((resolve) => {
      const tempAudio = new Audio(audioUrl);
      tempAudio.addEventListener('loadedmetadata', () => {
        resolve(Math.round(tempAudio.duration) || 0);
      });
      tempAudio.addEventListener('error', () => {
        resolve(0);
      });
    });

    const newTrack: Track = {
      id: 'upload-' + Date.now() + '-' + Math.floor(Math.random() * 1000),
      title,
      artist,
      album: 'Локальные файлы',
      duration,
      audioUrl,
      genre: customGenre || 'Audio',
      year: new Date().getFullYear(),
      format: validFormat,
      bitrate: Math.round((file.size * 8) / (duration || 180) / 1000) + ' kbps',
      plays: 0,
      isFavorite: false,
      addedAt: new Date().toISOString().split('T')[0],
      isLocalUpload: true,
      isOffline: true,
    };

    // Сохраняем аудиофайл в офлайн-кэш Cache API навсегда, чтобы он работал и после перезагрузки страницы
    await this.offlineService.saveBlobOffline(newTrack, file);

    this.tracks.update((current) => [newTrack, ...current]);
    this.persistTracks();
    return newTrack;
  }

  deleteTrack(trackId: string) {
    this.tracks.update((current) => current.filter((t) => t.id !== trackId));
    this.playlists.update((pls) =>
      pls.map((p) => ({
        ...p,
        trackIds: p.trackIds.filter((id) => id !== trackId),
      }))
    );
    this.persistTracks();
    this.persistPlaylists();
  }

  removeTrackFromPlaylist(playlistId: string, trackId: string) {
    this.playlists.update((pls) =>
      pls.map((p) => (p.id === playlistId ? { ...p, trackIds: p.trackIds.filter((id) => id !== trackId) } : p))
    );
    this.persistPlaylists();

    const otherTrackIds = new Set(this.playlists().flatMap((p) => p.trackIds));
    if (!otherTrackIds.has(trackId)) {
      this.tracks.update((tracks) => tracks.filter((t) => !(t.id === trackId && t.playlistOnly)));
      this.persistTracks();
    }
  }

  clearAllTracks() {
    this.tracks.set([]);
    this.playlists.update((pls) => pls.map((p) => ({ ...p, trackIds: [] })));
    this.persistTracks();
    this.persistPlaylists();
  }

  createPlaylist(title: string, description?: string): Playlist {
    const cleanTitle = title.trim();
    const coverText = cleanTitle.slice(0, 2).toUpperCase();
    const newPl: Playlist = {
      id: 'pl-' + Date.now(),
      title: cleanTitle,
      description: description?.trim() || 'Пользовательский плейлист',
      trackIds: [],
      coverText,
    };
    this.playlists.update((pls) => [...pls, newPl]);
    this.persistPlaylists();
    return newPl;
  }

  deletePlaylist(playlistId: string) {
    const pl = this.playlists().find((p) => p.id === playlistId);
    this.playlists.update((pls) => pls.filter((p) => p.id !== playlistId));
    if (this.activePlaylistId() === playlistId) {
      this.activePlaylistId.set(null);
      this.selectedView.set('all');
    }
    this.persistPlaylists();

    if (pl && pl.trackIds.length > 0) {
      const otherTrackIds = new Set(this.playlists().flatMap((p) => p.trackIds));
      this.tracks.update((tracks) =>
        tracks.filter((t) => !(t.playlistOnly && pl.trackIds.includes(t.id) && !otherTrackIds.has(t.id)))
      );
      this.persistTracks();
    }
  }

  toggleTrackInPlaylist(playlistId: string, trackId: string): boolean {
    let isAdded = false;
    this.playlists.update((pls) =>
      pls.map((p) => {
        if (p.id === playlistId) {
          if (p.trackIds.includes(trackId)) {
            isAdded = false;
            return { ...p, trackIds: p.trackIds.filter((id) => id !== trackId) };
          } else {
            isAdded = true;
            return { ...p, trackIds: [...p.trackIds, trackId] };
          }
        }
        return p;
      })
    );
    this.persistPlaylists();
    return isAdded;
  }

  exportLibrary() {
    const backupData = {
      version: 1,
      exportedAt: new Date().toISOString(),
      tracks: this.tracks().filter((t) => !t.audioUrl.startsWith('blob:')),
      playlists: this.playlists(),
      radioStations: this.radioStations(),
    };
    const jsonStr = JSON.stringify(backupData, null, 2);
    const blob = new Blob([jsonStr], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `recro-backup-${new Date().toISOString().split('T')[0]}.json`;
    a.click();
    URL.revokeObjectURL(url);
  }

  importLibrary(content: string): { tracksCount: number; playlistsCount: number; stationsCount: number } {
    let data: any;
    try {
      data = JSON.parse(content);
    } catch {
      throw new Error('Некорректный JSON файл бэкапа — не удалось разобрать файл');
    }
    if (!data || typeof data !== 'object') throw new Error('Некорректный формат файла бэкапа');

    let importedTracks = 0;
    if (Array.isArray(data.tracks)) {
      const current = this.tracks();
      const newTracks: Track[] = [];
      for (const t of data.tracks) {
        if (t && t.id && t.title && !current.some((x) => x.id === t.id || x.audioUrl === t.audioUrl)) {
          newTracks.push(t);
        }
      }
      if (newTracks.length > 0) {
        this.tracks.update((cur) => [...newTracks, ...cur]);
        this.persistTracks();
      }
      importedTracks = newTracks.length;
    }

    let importedPlaylists = 0;
    if (Array.isArray(data.playlists)) {
      const current = this.playlists();
      const newPlaylists: Playlist[] = [];
      for (const p of data.playlists) {
        if (p && p.id && p.title && !current.some((x) => x.id === p.id)) {
          newPlaylists.push(p);
        }
      }
      if (newPlaylists.length > 0) {
        this.playlists.update((cur) => [...cur, ...newPlaylists]);
        this.persistPlaylists();
      }
      importedPlaylists = newPlaylists.length;
    }

    let importedStations = 0;
    if (Array.isArray(data.radioStations)) {
      const current = this.radioStations();
      const newStations: RadioStation[] = [];
      for (const s of data.radioStations) {
        if (s && s.id && s.name && s.streamUrl && !current.some((x) => x.id === s.id || x.streamUrl === s.streamUrl)) {
          newStations.push(s);
        }
      }
      if (newStations.length > 0) {
        this.radioStations.update((cur) => [...cur, ...newStations]);
        this.persistStations();
      }
      importedStations = newStations.length;
    }

    return {
      tracksCount: importedTracks,
      playlistsCount: importedPlaylists,
      stationsCount: importedStations,
    };
  }

  async recordHistoryPlay(track: Track) {
    if (!this.isBackendOnline() || !this.authService.isAuthenticated()) return;
    try {
      await fetch(`${this.getBackendUrl()}/api/history`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          ...this.authService.getAuthHeaders(),
        },
        body: JSON.stringify({
          track_id: track.id,
          title: track.title,
          artist: track.artist,
          genre: track.genre,
          cover_url: track.coverUrl,
          duration: track.duration,
        }),
      });
    } catch {}
  }

  async getHistory(): Promise<HistoryItem[]> {
    if (!this.isBackendOnline() || !this.authService.isAuthenticated()) return [];
    try {
      const res = await fetch(`${this.getBackendUrl()}/api/history`, {
        headers: this.authService.getAuthHeaders(),
      });
      if (!res.ok) return [];
      return await res.json();
    } catch {
      return [];
    }
  }

  async clearHistory(): Promise<boolean> {
    if (!this.isBackendOnline() || !this.authService.isAuthenticated()) return false;
    try {
      const res = await fetch(`${this.getBackendUrl()}/api/history`, {
        method: 'DELETE',
        headers: this.authService.getAuthHeaders(),
      });
      return res.ok;
    } catch {
      return false;
    }
  }

  async getWrappedStats(): Promise<WrappedStats | null> {
    if (!this.isBackendOnline() || !this.authService.isAuthenticated()) return null;
    try {
      const res = await fetch(`${this.getBackendUrl()}/api/stats/wrapped`, {
        headers: this.authService.getAuthHeaders(),
      });
      if (!res.ok) return null;
      return await res.json();
    } catch {
      return null;
    }
  }

  /**
   * Вызывается после успешного логина.
   * Перезагружает библиотеку из namespace текущего пользователя,
   * затем синкается с облаком.
   */
  async onUserLoggedIn() {
    // Перезагружаем из namespace нового пользователя
    this.initLibrary();
    // Синкаемся с облаком (не force — уважаем локальные данные)
    await this.syncWithBackendOnStartup(false);
  }

  /**
   * Вызывается после выхода из аккаунта.
   * Переключается на guest namespace с дефолтными треками.
   */
  onUserLoggedOut() {
    this.tracks.set([...this.defaultTracks]);
    this.playlists.set([]);
    this.radioStations.set([...this.defaultRadioStations]);
    this.saveLocalWithoutCloudSync();
    this.isCloudSynced.set(false);
  }
}
