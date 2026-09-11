import { Injectable, signal, computed } from '@angular/core';
import { Track, Playlist, RadioStation } from '../models/track.model';

@Injectable({
  providedIn: 'root',
})
export class LibraryService {
  private activeBackendUrl = typeof window !== 'undefined' && window.location.protocol === 'https:'
    ? 'https://signal-audio-backend-production.up.railway.app'
    : 'http://localhost:8085';
  private readonly FALLBACK_BACKEND_URL = 'https://signal-audio-backend-production.up.railway.app';
  private readonly STORAGE_KEY_TRACKS = 'signal_music_user_tracks';
  private readonly STORAGE_KEY_FAVORITES = 'signal_music_favorites';
  private readonly STORAGE_KEY_PLAYLISTS = 'signal_music_playlists';
  private readonly STORAGE_KEY_STATIONS = 'signal_music_radio_stations';

  readonly defaultRadioStations: RadioStation[] = [
    {
      id: 'default-1',
      name: 'Record Chill-Out',
      streamUrl: 'https://radiorecord.hostingradio.ru/chil96.aacp',
      genre: 'Chillout / Lounge',
      country: 'RU',
      bitrate: '96k AAC',
    },
    {
      id: 'default-2',
      name: 'Europa Plus',
      streamUrl: 'https://ep128.hostingradio.ru:8030/ep128.mp3',
      genre: 'Pop / Top 40',
      country: 'RU',
      bitrate: '128k MP3',
    },
    {
      id: 'default-3',
      name: 'SomaFM: Groove Salad',
      streamUrl: 'https://ice4.somafm.com/groovesalad-128-mp3',
      genre: 'Ambient / Downtempo',
      country: 'US',
      bitrate: '128k Live',
    },
    {
      id: 'default-4',
      name: 'SomaFM: Drone Zone',
      streamUrl: 'https://ice2.somafm.com/dronezone-128-mp3',
      genre: 'Space / Atmospheric',
      country: 'US',
      bitrate: '128k Live',
    },
    {
      id: 'default-5',
      name: 'SomaFM: DEF CON Radio',
      streamUrl: 'https://ice6.somafm.com/defcon-128-mp3',
      genre: 'Electronic / Cyber',
      country: 'US',
      bitrate: '128k Live',
    },
    {
      id: 'default-6',
      name: 'Nightwave Plaza',
      streamUrl: 'https://plaza.one/mp3',
      genre: 'Vaporwave / Synth',
      country: 'Global',
      bitrate: '128k MP3',
    },
    {
      id: 'default-7',
      name: 'Jazz24',
      streamUrl: 'https://live.wostreaming.net/manifest/kplufm-jazz24aac-ibc1',
      genre: 'Classic Jazz',
      country: 'US',
      bitrate: '128k AAC',
    },
    {
      id: 'default-8',
      name: 'Rock Antenne',
      streamUrl: 'https://stream.rockantenne.de/rockantenne/stream/mp3',
      genre: 'Rock / Classic Rock',
      country: 'DE',
      bitrate: '192k MP3',
    },
    {
      id: 'default-9',
      name: 'Lofi 24/7 Stream',
      streamUrl: 'https://stream.zeno.fm/f3wvbbqmdg8uv',
      genre: 'Lo-Fi / Beats',
      country: 'Global',
      bitrate: '128k MP3',
    },
    {
      id: 'default-10',
      name: 'Record Deep',
      streamUrl: 'https://radiorecord.hostingradio.ru/deep96.aacp',
      genre: 'Deep House',
      country: 'RU',
      bitrate: '96k AAC',
    },
  ];

  readonly tracks = signal<Track[]>([]);
  readonly playlists = signal<Playlist[]>([]);
  readonly radioStations = signal<RadioStation[]>([]);
  readonly searchQuery = signal<string>('');
  readonly selectedGenre = signal<string>('all');
  readonly selectedView = signal<'all' | 'favorites' | 'uploads' | 'streams' | 'playlist'>('all');
  readonly activePlaylistId = signal<string | null>(null);

  readonly onlineSearchResults = signal<Track[]>([]);
  readonly isSearchingOnline = signal<boolean>(false);
  readonly isBackendOnline = signal<boolean>(false);

  readonly availableGenres = computed(() => {
    const all = this.tracks().map((t) => t.genre).filter(Boolean);
    return ['all', ...Array.from(new Set(all))];
  });

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

  getBackendUrl(): string {
    return this.activeBackendUrl;
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

    if (!savedStations || savedStations.length === 0) {
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

  async checkBackendHealth() {
    const savedBackend = typeof localStorage !== 'undefined' ? localStorage.getItem('signal_backend_url') : null;
    const isHttps = typeof window !== 'undefined' && window.location.protocol === 'https:';
    const urlsToTest: string[] = [];

    if (savedBackend) urlsToTest.push(savedBackend);
    if (isHttps) {
      urlsToTest.push(this.FALLBACK_BACKEND_URL);
    } else {
      urlsToTest.push('http://localhost:8085', this.FALLBACK_BACKEND_URL);
    }

    for (const testUrl of urlsToTest) {
      try {
        const res = await fetch(`${testUrl}/api/health`, { signal: AbortSignal.timeout(3000) });
        if (res.ok) {
          this.activeBackendUrl = testUrl;
          this.isBackendOnline.set(true);
          return;
        }
      } catch {}
    }

    this.isBackendOnline.set(false);
  }

  async searchOnline(query: string): Promise<Track[]> {
    const q = query.trim();
    if (!q) {
      this.onlineSearchResults.set([]);
      return [];
    }

    this.isSearchingOnline.set(true);
    try {
      const res = await fetch(`${this.activeBackendUrl}/api/search?q=${encodeURIComponent(q)}`);
      if (!res.ok) throw new Error('Search failed');

      const data: { id: string; title: string; artist: string; duration: number; audio_url: string; cover_url?: string }[] = await res.json();
      
      const tracks: Track[] = data.map((item) => ({
        id: 'yt-' + item.id,
        title: item.title,
        artist: item.artist,
        duration: Math.round(item.duration),
        audioUrl: item.audio_url,
        coverUrl: item.cover_url,
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

  async extractFromUrl(url: string): Promise<{ playlistTitle: string | null; tracks: Track[] }> {
    const targetUrl = url.trim();
    if (!targetUrl) return { playlistTitle: null, tracks: [] };

    const res = await fetch(`${this.activeBackendUrl}/api/extract?url=${encodeURIComponent(targetUrl)}`);
    if (!res.ok) throw new Error('Extract failed');

    const data: {
      playlist_title: string | null;
      tracks: { id: string; title: string; artist: string; duration: number; audio_url: string; cover_url?: string }[];
    } = await res.json();

    const tracks: Track[] = (data.tracks || []).map((item) => ({
      id: 'yt-' + item.id + '-' + Math.floor(Math.random() * 1000),
      title: item.title,
      artist: item.artist,
      duration: Math.round(item.duration),
      audioUrl: item.audio_url,
      coverUrl: item.cover_url,
      genre: 'YouTube',
      format: 'mp3',
      bitrate: '192 kbps',
      plays: 0,
      isFavorite: false,
      addedAt: new Date().toISOString().split('T')[0],
    }));

    return {
      playlistTitle: data.playlist_title,
      tracks,
    };
  }

  importPlaylist(title: string, tracks: Track[]): Playlist {
    const newTracks: Track[] = [];
    for (const t of tracks) {
      const exists = this.tracks().some((x) => x.id === t.id || x.audioUrl === t.audioUrl);
      if (!exists) {
        newTracks.push(t);
      }
    }

    if (newTracks.length > 0) {
      this.tracks.update((current) => [...newTracks, ...current]);
      this.persistTracks();
    }

    const playlist = this.createPlaylist(title, `Импортировано: ${tracks.length} треков`);
    const trackIds = tracks.map((t) => t.id);

    this.playlists.update((pls) =>
      pls.map((p) => (p.id === playlist.id ? { ...p, trackIds } : p))
    );
    this.persistPlaylists();

    return playlist;
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

  private persistStations() {
    try {
      localStorage.setItem(this.STORAGE_KEY_STATIONS, JSON.stringify(this.radioStations()));
    } catch (e) {
      console.warn('Failed to save stations to localStorage:', e);
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

  addMultipleTracks(tracks: Track[]) {
    const toAdd: Track[] = [];
    for (const t of tracks) {
      const exists = this.tracks().some((x) => x.id === t.id || x.audioUrl === t.audioUrl);
      if (!exists) toAdd.push(t);
    }
    if (toAdd.length > 0) {
      this.tracks.update((cur) => [...toAdd, ...cur]);
      this.persistTracks();
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
    a.download = `signal-backup-${new Date().toISOString().split('T')[0]}.json`;
    a.click();
    URL.revokeObjectURL(url);
  }

  importLibrary(content: string): { tracksCount: number; playlistsCount: number; stationsCount: number } {
    const data = JSON.parse(content);
    if (!data) throw new Error('Некорректный JSON файл бэкапа');

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
}

