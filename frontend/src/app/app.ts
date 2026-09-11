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
import { Track, Playlist } from './models/track.model';

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

  // UI State Signals
  readonly isAddModalOpen = signal<boolean>(false);
  readonly addModalTab = signal<'search' | 'url' | 'presets' | 'file'>('search');
  readonly isPlaylistModalOpen = signal<boolean>(false);
  readonly isQueueDrawerOpen = signal<boolean>(false);
  readonly activeTab = signal<'all' | 'favorites' | 'uploads' | 'streams' | 'playlist'>('all');
  readonly toastMessage = signal<string | null>(null);

  // Online Search in Modal
  readonly modalSearchInput = signal<string>('');

  // Active playlist details computed
  readonly currentPlaylist = computed(() => {
    const id = this.libraryService.activePlaylistId();
    if (!id) return null;
    return this.libraryService.playlists().find((p) => p.id === id) || null;
  });

  // Track Playlist Picker Menu
  readonly activePlaylistPickerTrackId = signal<string | null>(null);

  // Add Stream by URL Form State
  readonly inputUrl = signal<string>('');
  readonly inputTitle = signal<string>('');
  readonly inputArtist = signal<string>('');
  readonly inputGenre = signal<string>('Web Stream');
  readonly isLiveStreamCheckbox = signal<boolean>(false);
  readonly isUrlValidating = signal<boolean>(false);

  // Local File Upload Form State
  readonly uploadFile = signal<File | null>(null);
  readonly uploadFileName = signal<string>('');
  readonly uploadTitle = signal<string>('');
  readonly uploadArtist = signal<string>('');
  readonly uploadGenre = signal<string>('Electronic');
  readonly isDragging = signal<boolean>(false);

  // Create Playlist Modal State
  readonly playlistTitleInput = signal<string>('');
  readonly playlistDescInput = signal<string>('');

  // Scrubber Dragging State
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

  // Formatted Time Helper
  formatTime(seconds: number): string {
    if (isNaN(seconds) || seconds < 0 || !isFinite(seconds)) return '0:00';
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins}:${secs < 10 ? '0' : ''}${secs}`;
  }

  ngOnInit() {
    this.libraryService.checkBackendHealth();
  }

  // Keyboard Shortcuts
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

  // Navigation
  setView(view: 'all' | 'favorites' | 'uploads' | 'streams' | 'playlist', playlistId?: string) {
    this.activeTab.set(view);
    this.libraryService.selectedView.set(view);
    if (playlistId) {
      this.libraryService.activePlaylistId.set(playlistId);
    } else {
      this.libraryService.activePlaylistId.set(null);
    }
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

  // Online Search Handlers (Rust Gateway)
  async triggerOnlineSearch() {
    const q = this.modalSearchInput().trim();
    if (!q) return;
    await this.libraryService.searchOnline(q);
  }

  openOnlineSearchWithQuery(query: string) {
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

  // Playlist Management Modal
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

  // URL Stream Add
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

  // Preset Radio Add
  async addPresetStream(preset: { title: string; artist: string; genre: string; url: string; bitrate: string }) {
    const track = await this.libraryService.addStreamTrack(
      preset.url,
      preset.title,
      preset.artist,
      preset.genre,
      true
    );
    this.showToast(`Радиостанция "${preset.title}" добавлена`);
    this.isAddModalOpen.set(false);
    this.audioService.playTrack(track, this.libraryService.tracks());
  }

  // Local File Upload
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
}
