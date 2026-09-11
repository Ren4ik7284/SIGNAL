import { Injectable, signal, computed } from '@angular/core';
import { Track, Playlist } from '../models/track.model';

@Injectable({
  providedIn: 'root',
})
export class LibraryService {
  private readonly BACKEND_URL = 'http://localhost:8085';
  private readonly STORAGE_KEY_TRACKS = 'signal_music_user_tracks';
  private readonly STORAGE_KEY_FAVORITES = 'signal_music_favorites';
  private readonly STORAGE_KEY_PLAYLISTS = 'signal_music_playlists';

  readonly presetStreams: { title: string; artist: string; genre: string; url: string; bitrate: string }[] = [
    {
      title: 'Drone Zone 24/7',
      artist: 'SomaFM Stream',
      genre: 'Ambient / Space',
      url: 'https://ice2.somafm.com/dronezone-128-mp3',
      bitrate: '128k Live',
    },
    {
      title: 'Groove Salad Chill',
      artist: 'SomaFM Stream',
      genre: 'Downtempo',
      url: 'https://ice4.somafm.com/groovesalad-128-mp3',
      bitrate: '128k Live',
    },
    {
      title: 'DEF CON Hacker Radio',
      artist: 'SomaFM Stream',
      genre: 'Electronic',
      url: 'https://ice6.somafm.com/defcon-128-mp3',
      bitrate: '128k Live',
    },
    {
      title: 'Secret Agent 007',
      artist: 'SomaFM Stream',
      genre: 'Downtempo / Spy',
      url: 'https://ice2.somafm.com/secretagent-128-mp3',
      bitrate: '128k Live',
    },
  ];

  // State Signals
  readonly tracks = signal<Track[]>([]);
  readonly playlists = signal<Playlist[]>([]);
  readonly searchQuery = signal<string>('');
  readonly selectedGenre = signal<string>('all');
  readonly selectedView = signal<'all' | 'favorites' | 'uploads' | 'streams' | 'playlist'>('all');
  readonly activePlaylistId = signal<string | null>(null);

  // Online Search (via Rust Axum + yt-dlp backend)
  readonly onlineSearchResults = signal<Track[]>([]);
  readonly isSearchingOnline = signal<boolean>(false);
  readonly isBackendOnline = signal<boolean>(false);

  // Available Genres
  readonly availableGenres = computed(() => {
    const all = this.tracks().map((t) => t.genre).filter(Boolean);
    return ['all', ...Array.from(new Set(all))];
  });

  // Filtered tracks
  readonly filteredTracks = computed(() => {
    const query = this.searchQuery().trim().toLowerCase();
    const genre = this.selectedGenre();
    const view = this.selectedView();
    const playlistId = this.activePlaylistId();

    return this.tracks().filter((track) => {
      if (view === 'favorites' && !track.isFavorite) return false;
      if (view === 'uploads' && !track.isLocalUpload) return false;
      if (view === 'streams' && !track.isLiveStream && track.format !== 'stream') return false;
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

  constructor() {
    this.initLibrary();
    this.checkBackendHealth();
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

    const processedTracks = savedTracks.map((t) => ({
      ...t,
      isFavorite: favIds.includes(t.id),
    }));

    this.tracks.set(processedTracks);
    this.playlists.set(savedPlaylists);
  }

  async checkBackendHealth() {
    try {
      const res = await fetch(`${this.BACKEND_URL}/api/health`);
      if (res.ok) {
        this.isBackendOnline.set(true);
      }
    } catch {
      this.isBackendOnline.set(false);
    }
  }

  // Search Online Music via Rust Backend
  async searchOnline(query: string): Promise<Track[]> {
    const q = query.trim();
    if (!q) {
      this.onlineSearchResults.set([]);
      return [];
    }

    this.isSearchingOnline.set(true);
    try {
      const res = await fetch(`${this.BACKEND_URL}/api/search?q=${encodeURIComponent(q)}`);
      if (!res.ok) throw new Error('Search failed');

      const data: { id: string; title: string; artist: string; duration: number; audio_url: string; cover_url?: string }[] = await res.json();
      
      const tracks: Track[] = data.map((item) => ({
        id: 'online-' + item.id,
        title: item.title,
        artist: item.artist,
        duration: Math.round(item.duration),
        audioUrl: item.audio_url,
        coverUrl: item.cover_url,
        genre: 'Online Music',
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

  private persistTracks() {
    const persistable = this.tracks().filter((t) => !t.audioUrl.startsWith('blob:'));
    try {
      localStorage.setItem(this.STORAGE_KEY_TRACKS, JSON.stringify(persistable));
    } catch (e) {
      console.warn('Failed to save tracks to localStorage:', e);
    }
  }

  private persistPlaylists() {
    try {
      localStorage.setItem(this.STORAGE_KEY_PLAYLISTS, JSON.stringify(this.playlists()));
    } catch (e) {
      console.warn('Failed to save playlists to localStorage:', e);
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
    localStorage.setItem(this.STORAGE_KEY_FAVORITES, JSON.stringify(favs));
  }

  addTrackToLibrary(track: Track) {
    const exists = this.tracks().some((t) => t.id === track.id || t.audioUrl === track.audioUrl);
    if (!exists) {
      this.tracks.update((cur) => [track, ...cur]);
      this.persistTracks();
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

    let duration = 0;
    if (!isLive) {
      duration = await new Promise<number>((resolve) => {
        const probeAudio = new Audio(cleanUrl);
        probeAudio.addEventListener('loadedmetadata', () => {
          const d = probeAudio.duration;
          resolve(isFinite(d) && d > 0 ? Math.round(d) : 0);
        });
        probeAudio.addEventListener('error', () => resolve(0));
        setTimeout(() => resolve(0), 2500);
      });
    }

    const newTrack: Track = {
      id: 'stream-' + Date.now() + '-' + Math.floor(Math.random() * 1000),
      title,
      artist,
      album: isLive ? 'Live Radio Stations' : 'Web Streams',
      duration: duration || 0,
      audioUrl: cleanUrl,
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
    };

    this.tracks.update((current) => [newTrack, ...current]);
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
    this.playlists.update((pls) => pls.filter((p) => p.id !== playlistId));
    if (this.activePlaylistId() === playlistId) {
      this.activePlaylistId.set(null);
      this.selectedView.set('all');
    }
    this.persistPlaylists();
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
}
