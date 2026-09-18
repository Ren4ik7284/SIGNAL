import {
  Component,
  OnInit,
  inject,
  signal,
  computed,
  HostListener,
} from '@angular/core';
import { CommonModule } from '@angular/common';
import { FormsModule } from '@angular/forms';
import { AudioService } from './services/audio.service';
import { LibraryService } from './services/library.service';
import { Track, Playlist, RadioStation } from './models/track.model';

@Component({
  selector: 'app-root',
  standalone: true,
  imports: [CommonModule, FormsModule],
  templateUrl: './app.html',
  styleUrl: './app.scss',
})
export class App implements OnInit {
  readonly audioService = inject(AudioService);
  readonly libraryService = inject(LibraryService);

  readonly isAddModalOpen = signal<boolean>(false);
  readonly addModalTab = signal<'youtube' | 'search' | 'radio' | 'url' | 'file'>('youtube');
  readonly isPlaylistModalOpen = signal<boolean>(false);
  readonly isQueueDrawerOpen = signal<boolean>(false);
  readonly activeTab = signal<'all' | 'favorites' | 'uploads' | 'streams' | 'playlist'>('all');
  readonly toastMessage = signal<string | null>(null);

  readonly isMobilePlayerExpanded = signal<boolean>(false);
  readonly isMobilePlaylistsOpen = signal<boolean>(false);

  readonly isMobileDevice = signal<boolean>(false);
  readonly canInstallPwa = signal<boolean>(false);
  readonly isPwaModalOpen = signal<boolean>(false);
  readonly isIos = signal<boolean>(false);
  private deferredPrompt: any = null;

  readonly modalSearchInput = signal<string>('');

  readonly currentPlaylist = computed(() => {
    const id = this.libraryService.activePlaylistId();
    if (!id) return null;
    return this.libraryService.playlists().find((p) => p.id === id) || null;
  });

  readonly activePlaylistPickerTrackId = signal<string | null>(null);

  readonly youtubeUrlInput = signal<string>('');
  readonly isExtractingUrl = signal<boolean>(false);
  readonly extractedResult = signal<{ playlistTitle: string | null; tracks: Track[] } | null>(null);
  readonly extractError = signal<string | null>(null);

  readonly radioSearchInput = signal<string>('');
  readonly isSearchingRadio = signal<boolean>(false);
  readonly radioSearchResults = signal<RadioStation[]>([]);
  readonly isAddStationModalOpen = signal<boolean>(false);
  readonly newStationName = signal<string>('');
  readonly newStationUrl = signal<string>('');
  readonly newStationGenre = signal<string>('Pop');

  readonly inputUrl = signal<string>('');
  readonly inputTitle = signal<string>('');
  readonly inputArtist = signal<string>('');
  readonly inputGenre = signal<string>('Web Stream');
  readonly isLiveStreamCheckbox = signal<boolean>(false);
  readonly isUrlValidating = signal<boolean>(false);

  readonly uploadFile = signal<File | null>(null);
  readonly uploadFileName = signal<string>('');
  readonly uploadTitle = signal<string>('');
  readonly uploadArtist = signal<string>('');
  readonly uploadGenre = signal<string>('Electronic');
  readonly isDragging = signal<boolean>(false);

  readonly playlistTitleInput = signal<string>('');
  readonly playlistDescInput = signal<string>('');

  readonly isScrubbing = signal<boolean>(false);
  readonly scrubTime = signal<number>(0);

  onScrubberInput(val: number) {
    this.isScrubbing.set(true);
    this.scrubTime.set(val);
  }

  onScrubberChange(val: number) {
    this.isScrubbing.set(false);
    this.audioService.seek(val);
  }

  formatTime(seconds: number): string {
    if (isNaN(seconds) || seconds < 0 || !isFinite(seconds)) return '0:00';
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins}:${secs < 10 ? '0' : ''}${secs}`;
  }

  ngOnInit() {
    this.libraryService.checkBackendHealth();

    if (typeof window !== 'undefined') {
      const isIosDevice = /iPhone|iPad|iPod/i.test(navigator.userAgent);
      this.isIos.set(isIosDevice);

      const isMobile = isIosDevice || /Android|webOS|BlackBerry|IEMobile|Opera Mini/i.test(navigator.userAgent) || window.innerWidth <= 768;
      this.isMobileDevice.set(isMobile);

      window.addEventListener('beforeinstallprompt', (e: Event) => {
        if (isMobile) {
          e.preventDefault();
          this.deferredPrompt = e;
          this.canInstallPwa.set(true);
        } else {
          e.preventDefault();
          this.deferredPrompt = null;
          this.canInstallPwa.set(false);
        }
      });
    }
  }

  openPwaInstallModal() {
    if (this.deferredPrompt) {
      this.deferredPrompt.prompt();
      this.deferredPrompt.userChoice.then((choice: any) => {
        if (choice && choice.outcome === 'accepted') {
          this.canInstallPwa.set(false);
          this.showToast('Приложение установлено');
        }
        this.deferredPrompt = null;
      });
      return;
    }
    this.isPwaModalOpen.set(true);
  }

  installPwa() {
    this.openPwaInstallModal();
  }

  @HostListener('window:keydown', ['$event'])
  handleKeyboardEvent(event: KeyboardEvent) {
    const target = event.target as HTMLElement;
    if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA')) {
      return;
    }

    if (event.code === 'Space') {
      event.preventDefault();
      this.audioService.togglePlay();
    } else if (event.code === 'ArrowRight') {
      event.preventDefault();
      this.audioService.skipBy(5);
      this.showToast('Перемотка +5с');
    } else if (event.code === 'ArrowLeft') {
      event.preventDefault();
      this.audioService.skipBy(-5);
      this.showToast('Перемотка -5с');
    } else if (event.code === 'ArrowUp') {
      event.preventDefault();
      this.audioService.setVolume(this.audioService.volume() + 0.05);
    } else if (event.code === 'ArrowDown') {
      event.preventDefault();
      this.audioService.setVolume(this.audioService.volume() - 0.05);
    } else if (event.key === 'm' || event.key === 'ь') {
      event.preventDefault();
      this.audioService.toggleMute();
      this.showToast(this.audioService.isMuted() ? 'Звук выключен' : 'Звук включен');
    } else if (event.key === 'l' || event.key === 'д') {
      const current = this.audioService.currentTrack();
      if (current) {
        event.preventDefault();
        this.libraryService.toggleFavorite(current.id);
        this.showToast(current.isFavorite ? 'Удалено из избранного' : 'Добавлено в избранное');
      }
    }
  }

  showToast(msg: string) {
    this.toastMessage.set(msg);
    setTimeout(() => {
      if (this.toastMessage() === msg) {
        this.toastMessage.set(null);
      }
    }, 3000);
  }

  setView(view: 'all' | 'favorites' | 'uploads' | 'streams' | 'playlist', playlistId?: string) {
    this.activeTab.set(view);
    this.libraryService.selectedView.set(view);
    if (playlistId) {
      this.libraryService.activePlaylistId.set(playlistId);
    } else {
      this.libraryService.activePlaylistId.set(null);
    }
    this.isMobilePlaylistsOpen.set(false);
  }

  selectGenre(genre: string) {
    this.libraryService.selectedGenre.set(genre);
  }

  playTrack(track: Track) {
    this.audioService.playTrack(track, this.libraryService.filteredTracks());
  }

  deleteTrack(track: Track) {
    this.libraryService.deleteTrack(track.id);
    this.showToast(`Трек "${track.title}" удален`);
  }

  async triggerOnlineSearch() {
    const q = this.modalSearchInput().trim();
    if (!q) return;
    await this.libraryService.searchOnline(q);
  }

  openOnlineSearchWithQuery(query: string) {
    if (query.startsWith('http://') || query.startsWith('https://')) {
      this.youtubeUrlInput.set(query);
      this.addModalTab.set('youtube');
      this.isAddModalOpen.set(true);
      this.extractYouTubeUrl();
      return;
    }
    this.modalSearchInput.set(query);
    this.addModalTab.set('search');
    this.isAddModalOpen.set(true);
    this.triggerOnlineSearch();
  }

  playOnlineTrack(track: Track) {
    this.libraryService.addTrackToLibrary(track);
    this.audioService.playTrack(track, this.libraryService.tracks());
    this.showToast(`Воспроизведение: ${track.title}`);
  }

  addOnlineTrackToLib(track: Track) {
    this.libraryService.addTrackToLibrary(track);
    this.showToast(`Трек "${track.title}" сохранен в медиатеку`);
  }

  async extractYouTubeUrl() {
    const url = this.youtubeUrlInput().trim();
    if (!url) return;

    this.isExtractingUrl.set(true);
    this.extractError.set(null);
    this.extractedResult.set(null);

    try {
      const res = await this.libraryService.extractFromUrl(url);
      if (!res.tracks || res.tracks.length === 0) {
        this.extractError.set('Не удалось извлечь аудио по этой ссылке. Проверьте URL.');
      } else {
        this.extractedResult.set(res);
      }
    } catch {
      this.extractError.set('Ошибка соединения с бэкендом. Убедитесь, что бэкенд запущен.');
    } finally {
      this.isExtractingUrl.set(false);
    }
  }

  importAllExtractedTracks() {
    const data = this.extractedResult();
    if (!data || data.tracks.length === 0) return;

    const title = data.playlistTitle || 'YouTube Плейлист';
    this.libraryService.importPlaylist(title, data.tracks);
    this.showToast(`Импортировано ${data.tracks.length} треков в плейлист "${title}"`);
    this.isAddModalOpen.set(false);
    this.youtubeUrlInput.set('');
    this.extractedResult.set(null);
  }

  addExtractedTracksToLibraryOnly() {
    const data = this.extractedResult();
    if (!data || data.tracks.length === 0) return;

    this.libraryService.addMultipleTracks(data.tracks);
    this.showToast(`Добавлено ${data.tracks.length} треков в медиатеку`);
    this.isAddModalOpen.set(false);
    this.youtubeUrlInput.set('');
    this.extractedResult.set(null);
  }

  playExtractedTrackNow(track: Track) {
    this.libraryService.addTrackToLibrary(track);
    this.audioService.playTrack(track, this.libraryService.tracks());
    this.showToast(`Воспроизведение: ${track.title}`);
  }

  async searchRadio() {
    const q = this.radioSearchInput().trim();
    if (!q) {
      this.radioSearchResults.set([]);
      return;
    }

    this.isSearchingRadio.set(true);
    try {
      const results = await this.libraryService.searchRadioBrowser(q);
      this.radioSearchResults.set(results);
    } catch {
      this.radioSearchResults.set([]);
    } finally {
      this.isSearchingRadio.set(false);
    }
  }

  playRadioStation(station: RadioStation) {
    const track = this.libraryService.createTrackFromStation(station);
    this.audioService.playTrack(track, this.libraryService.tracks());
    this.showToast(`Радио: ${station.name}`);
  }

  addRadioStationToMyList(station: RadioStation) {
    this.libraryService.addRadioStation({
      name: station.name,
      streamUrl: station.streamUrl,
      genre: station.genre,
      country: station.country,
      bitrate: station.bitrate,
    });
    this.showToast(`Станция "${station.name}" сохранена`);
  }

  deleteRadioStation(stationId: string) {
    this.libraryService.removeRadioStation(stationId);
    this.showToast('Радиостанция удалена');
  }

  resetRadioStations() {
    this.libraryService.resetDefaultStations();
    this.showToast('Список радиостанций сброшен по умолчанию');
  }

  saveCustomStation() {
    const name = this.newStationName().trim();
    const url = this.newStationUrl().trim();
    if (!url) return;

    this.libraryService.addRadioStation({
      name: name || 'Мое радио',
      streamUrl: url,
      genre: this.newStationGenre().trim() || 'Custom',
    });

    this.isAddStationModalOpen.set(false);
    this.newStationName.set('');
    this.newStationUrl.set('');
    this.showToast(`Станция "${name || 'Мое радио'}" добавлена`);
  }

  openCreatePlaylistModal() {
    this.playlistTitleInput.set('');
    this.playlistDescInput.set('');
    this.isPlaylistModalOpen.set(true);
  }

  submitCreatePlaylist() {
    const title = this.playlistTitleInput().trim();
    if (!title) return;

    const desc = this.playlistDescInput().trim();
    const pl = this.libraryService.createPlaylist(title, desc);
    this.isPlaylistModalOpen.set(false);
    this.showToast(`Плейлист "${pl.title}" создан`);
    this.setView('playlist', pl.id);
  }

  deleteCurrentPlaylist() {
    const pl = this.currentPlaylist();
    if (!pl) return;
    this.libraryService.deletePlaylist(pl.id);
    this.showToast(`Плейлист "${pl.title}" удален`);
  }

  toggleTrackInPlaylist(playlist: Playlist, track: Track) {
    const isAdded = this.libraryService.toggleTrackInPlaylist(playlist.id, track.id);
    this.showToast(
      isAdded
        ? `Трек добавлен в "${playlist.title}"`
        : `Трек убран из "${playlist.title}"`
    );
    this.activePlaylistPickerTrackId.set(null);
  }

  async submitUrlTrack() {
    const url = this.inputUrl().trim();
    if (!url) return;

    this.isUrlValidating.set(true);
    try {
      const track = await this.libraryService.addStreamTrack(
        url,
        this.inputTitle() || undefined,
        this.inputArtist() || undefined,
        this.inputGenre() || undefined,
        this.isLiveStreamCheckbox()
      );

      this.showToast(`Поток "${track.title}" добавлен`);
      this.isAddModalOpen.set(false);
      this.inputUrl.set('');
      this.inputTitle.set('');
      this.inputArtist.set('');

      this.audioService.playTrack(track, this.libraryService.tracks());
    } catch {
      this.showToast('Ошибка подключения к потоку');
    } finally {
      this.isUrlValidating.set(false);
    }
  }

  onFileSelected(event: Event) {
    const input = event.target as HTMLInputElement;
    if (input.files && input.files.length > 0) {
      this.prepareUpload(input.files[0]);
    }
  }

  onDragOver(e: DragEvent) {
    e.preventDefault();
    this.isDragging.set(true);
  }

  onDragLeave(e: DragEvent) {
    e.preventDefault();
    this.isDragging.set(false);
  }

  onDrop(e: DragEvent) {
    e.preventDefault();
    this.isDragging.set(false);
    if (e.dataTransfer && e.dataTransfer.files.length > 0) {
      this.prepareUpload(e.dataTransfer.files[0]);
    }
  }

  private prepareUpload(file: File) {
    this.uploadFile.set(file);
    this.uploadFileName.set(file.name);
    const cleanName = file.name.replace(/\.[^/.]+$/, '');
    if (cleanName.includes(' - ')) {
      const parts = cleanName.split(' - ');
      this.uploadArtist.set(parts[0].trim());
      this.uploadTitle.set(parts.slice(1).join(' - ').trim());
    } else {
      this.uploadTitle.set(cleanName);
      this.uploadArtist.set('Локальный файл');
    }
  }

  async submitUpload() {
    const file = this.uploadFile();
    if (!file) return;

    const track = await this.libraryService.addUploadedFile(
      file,
      this.uploadTitle() || undefined,
      this.uploadArtist() || undefined,
      this.uploadGenre() || undefined
    );

    this.showToast(`Файл "${track.title}" добавлен`);
    this.isAddModalOpen.set(false);
    this.uploadFile.set(null);
    this.uploadFileName.set('');

    this.audioService.playTrack(track, this.libraryService.tracks());
  }

  exportLibrary() {
    this.libraryService.exportLibrary();
    this.showToast('Медиатека экспортирована в файл');
  }

  onBackupFileSelected(event: Event) {
    const input = event.target as HTMLInputElement;
    if (input.files && input.files.length > 0) {
      const file = input.files[0];
      const reader = new FileReader();
      reader.onload = () => {
        try {
          const res = this.libraryService.importLibrary(reader.result as string);
          this.showToast(
            `Импортировано: ${res.tracksCount} треков, ${res.playlistsCount} плейлистов, ${res.stationsCount} радио`
          );
        } catch {
          this.showToast('Ошибка импорта: некорректный файл бэкапа');
        }
      };
      reader.readAsText(file);
      input.value = '';
    }
  }
}
