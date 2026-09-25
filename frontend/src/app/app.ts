import {
  Component,
  OnInit,
  inject,
  signal,
  computed,
  HostListener,
  ViewEncapsulation,
} from '@angular/core';
import { CommonModule } from '@angular/common';
import { FormsModule } from '@angular/forms';
import { AudioService } from './services/audio.service';
import { LibraryService, ExtractedResult } from './services/library.service';
import { Track, Playlist, RadioStation, MixMood, MixSource, MixLanguage, MixConfig, DEFAULT_MIX_CONFIG } from './models/track.model';
import { HeaderComponent } from './components/header/header.component';
import { SidebarComponent } from './components/sidebar/sidebar.component';
import { PlayerBarComponent } from './components/player-bar/player-bar.component';
import { VisualizerComponent } from './components/visualizer/visualizer.component';
import { OfflineService } from './services/offline.service';
import { AuthService, HistoryItem, WrappedStats } from './services/auth.service';
import { RecommendationService } from './services/recommendation.service';

declare global {
  interface Window {
    google?: any;
  }
}

@Component({
  selector: 'app-root',
  standalone: true,
  imports: [CommonModule, FormsModule, HeaderComponent, SidebarComponent, PlayerBarComponent, VisualizerComponent],
  templateUrl: './app.html',
  styleUrl: './app.scss',
  encapsulation: ViewEncapsulation.None,
})
export class App implements OnInit {
  readonly audioService = inject(AudioService);
  readonly libraryService = inject(LibraryService);
  readonly offlineService = inject(OfflineService);
  readonly authService = inject(AuthService);
  readonly recService = inject(RecommendationService);

  readonly isQuickStartMixModalOpen = signal<boolean>(false);
  readonly isMixSettingsModalOpen = signal<boolean>(false);

  readonly isAuthModalOpen = signal<boolean>(false);
  readonly authModalTab = signal<'login' | 'register'>('login');
  readonly authUsernameInput = signal<string>('');
  readonly authPasswordInput = signal<string>('');
  readonly showPassword = signal<boolean>(false);
  readonly isGoogleConfigOpen = signal<boolean>(false);
  readonly customGoogleClientIdInput = signal<string>('');

  readonly isWrappedModalOpen = signal<boolean>(false);
  readonly wrappedStats = signal<WrappedStats | null>(null);
  readonly isLoadingWrapped = signal<boolean>(false);

  readonly isHistoryModalOpen = signal<boolean>(false);
  readonly historyList = signal<HistoryItem[]>([]);
  readonly isLoadingHistory = signal<boolean>(false);

  readonly isAddModalOpen = signal<boolean>(false);
  readonly addModalTab = signal<'youtube' | 'search' | 'radio' | 'url' | 'file'>('youtube');
  readonly isPlaylistModalOpen = signal<boolean>(false);
  readonly isQueueDrawerOpen = signal<boolean>(false);
  readonly activeTab = signal<'all' | 'favorites' | 'uploads' | 'streams' | 'playlist' | 'offline'>('all');
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
  readonly extractedResult = signal<ExtractedResult | null>(null);
  readonly extractMode = signal<'single' | 'playlist'>('single');
  readonly selectedExtractedTrackIds = signal<Set<string>>(new Set());
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

  readonly isAddToPlaylistModalOpen = signal<boolean>(false);
  readonly targetTrackForPlaylist = signal<Track | null>(null);
  readonly isRefreshingLibrary = signal<boolean>(false);

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
    this.authService.fetchAuthConfig(this.libraryService.getBackendUrl());

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
    } else if (event.key === 'v' || event.key === 'V' || event.key === 'м' || event.key === 'М') {
      event.preventDefault();
      this.audioService.toggleVisualizer();
      this.showToast(this.audioService.isVisualizerOpen() ? 'Визуализатор открыт' : 'Визуализатор закрыт');
    } else if (event.key === 'Escape') {
      if (this.audioService.isVisualizerOpen()) {
        this.audioService.closeVisualizer();
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

  openAuthModal(tab: 'login' | 'register' = 'login') {
    this.authModalTab.set(tab);
    this.authUsernameInput.set('');
    this.authPasswordInput.set('');
    this.authService.authError.set(null);
    this.isGoogleConfigOpen.set(false);
    this.customGoogleClientIdInput.set(this.authService.googleClientId() || '');
    this.isAuthModalOpen.set(true);

    this.renderGoogleButton();
  }

  toggleGoogleConfig() {
    this.isGoogleConfigOpen.update((v) => !v);
  }

  saveCustomGoogleClientId() {
    const val = this.customGoogleClientIdInput().trim();
    if (!val) {
      this.authService.authError.set('Введите Google Client ID');
      return;
    }
    this.authService.setCustomGoogleClientId(val);
    this.isGoogleConfigOpen.set(false);
    this.showToast('Google Client ID сохранён');
    this.renderGoogleButton();
  }

  async renderGoogleButton() {
    const backendUrl = this.libraryService.getBackendUrl();
    await this.authService.fetchAuthConfig(backendUrl);

    const clientId = this.authService.googleClientId();
    if (!clientId) return;

    const loaded = await this.ensureGoogleScriptLoaded();
    if (!loaded || !window.google?.accounts?.id) {
      return;
    }

    try {
      window.google.accounts.id.initialize({
        client_id: clientId,
        callback: async (response: any) => {
          if (response && response.credential) {
            const ok = await this.authService.loginWithGoogle(
              this.libraryService.getBackendUrl(),
              response.credential
            );
            if (ok) {
              this.isAuthModalOpen.set(false);
              const username = this.authService.currentUser()?.username || 'пользователь';
              this.showToast(`Вход выполнен! С возвращением, ${username}!`);
              await this.libraryService.onUserLoggedIn();
            }
          }
        },
        auto_select: false,
        cancel_on_tap_outside: true,
        ux_mode: 'popup',
        context: 'signin',
      });

      setTimeout(() => {
        const slot = document.getElementById('google-btn-slot');
        if (slot && window.google?.accounts?.id) {
          slot.innerHTML = '';
          const buttonWidth = typeof window !== 'undefined' ? Math.min(320, window.innerWidth - 64) : 280;
          window.google.accounts.id.renderButton(slot, {
            type: 'standard',
            theme: 'filled_black',
            size: 'large',
            text: 'signin_with',
            shape: 'rectangular',
            logo_alignment: 'left',
            width: buttonWidth,
          });
        }
      }, 60);
    } catch (err) {
      console.warn('[SIGNAL GSI] Ошибка инициализации кнопки Google:', err);
    }
  }

  private ensureGoogleScriptLoaded(): Promise<boolean> {
    if (typeof window === 'undefined') return Promise.resolve(false);
    if (window.google?.accounts?.id) return Promise.resolve(true);

    return new Promise((resolve) => {
      let attempts = 0;
      const timer = setInterval(() => {
        attempts++;
        if (window.google?.accounts?.id) {
          clearInterval(timer);
          resolve(true);
        } else if (attempts >= 25) {
          clearInterval(timer);
          resolve(false);
        }
      }, 100);
    });
  }

  async submitAuth() {
    const backendUrl = this.libraryService.getBackendUrl();

    if (this.authModalTab() === 'login') {
      const loginVal = this.authUsernameInput().trim();
      const pass = this.authPasswordInput().trim();

      if (!loginVal || !pass) {
        this.authService.authError.set('Заполните логин и пароль');
        return;
      }

      if (this.authService.hasWhitespace(loginVal)) {
        this.authService.authError.set('Логин не должен содержать пробелы');
        return;
      }

      const ok = await this.authService.login(backendUrl, loginVal, pass);
      if (ok) {
        this.authPasswordInput.set('');
        this.isAuthModalOpen.set(false);
        this.showToast(`Добро пожаловать, ${this.authService.currentUser()?.username || loginVal}!`);
        await this.libraryService.onUserLoggedIn();
      }
    } else if (this.authModalTab() === 'register') {
      const username = this.authUsernameInput().trim();
      const pass = this.authPasswordInput().trim();

      if (!username || !pass) {
        this.authService.authError.set('Заполните имя пользователя и пароль');
        return;
      }

      if (this.authService.hasWhitespace(username)) {
        this.authService.authError.set('Имя пользователя не должно содержать пробелы');
        return;
      }

      if (!this.authService.isValidUsername(username)) {
        this.authService.authError.set('Имя пользователя должно быть от 3 до 30 символов (только латиница, цифры, _ и -)');
        return;
      }

      if (!this.authService.isValidPassword(pass)) {
        this.authService.authError.set('Пароль должен содержать от 6 до 72 символов');
        return;
      }

      const ok = await this.authService.register(backendUrl, username, pass);
      if (ok) {
        this.authPasswordInput.set('');
        this.isAuthModalOpen.set(false);
        this.showToast(`Регистрация успешна! Добро пожаловать, ${username}!`);
        await this.libraryService.onUserLoggedIn();
      }
    }
  }

  async openWrappedModal() {
    if (!this.authService.isAuthenticated()) {
      this.showToast('Войдите в аккаунт для просмотра Recro Wrapped');
      this.openAuthModal('login');
      return;
    }

    this.isLoadingWrapped.set(true);
    this.isWrappedModalOpen.set(true);
    try {
      const stats = await this.libraryService.getWrappedStats();
      this.wrappedStats.set(stats);
    } catch {
      this.showToast('Не удалось загрузить статистику');
    } finally {
      this.isLoadingWrapped.set(false);
    }
  }

  async openHistoryModal() {
    if (!this.authService.isAuthenticated()) {
      this.showToast('Войдите в аккаунт для просмотра истории');
      this.openAuthModal('login');
      return;
    }

    this.isLoadingHistory.set(true);
    this.isHistoryModalOpen.set(true);
    try {
      const history = await this.libraryService.getHistory();
      this.historyList.set(history);
    } catch {
      this.showToast('Не удалось загрузить историю');
    } finally {
      this.isLoadingHistory.set(false);
    }
  }

  async clearListeningHistory() {
    if (confirm('Очистить всю историю прослушиваний?')) {
      const ok = await this.libraryService.clearHistory();
      if (ok) {
        this.historyList.set([]);
        this.showToast('История прослушиваний очищена');
      }
    }
  }

  playFromHistory(item: HistoryItem) {
    const existing = this.libraryService.tracks().find((t) => t.id === item.track_id);
    if (existing) {
      this.playTrack(existing);
    } else {
      const tempTrack: Track = {
        id: item.track_id,
        title: item.track_title,
        artist: item.track_artist,
        duration: item.duration || 0,
        audioUrl: `${this.libraryService.getBackendUrl()}/api/stream?id=${encodeURIComponent(item.track_id.replace(/^yt-/, ''))}&title=${encodeURIComponent(item.track_title)}&artist=${encodeURIComponent(item.track_artist)}`,
        coverUrl: item.cover_url,
        genre: item.track_genre || 'Music',
        format: 'mp3',
        plays: 1,
        isFavorite: false,
        addedAt: new Date(item.played_at * 1000).toISOString().split('T')[0],
      };
      this.libraryService.addTrackToLibrary(tempTrack);
      this.playTrack(tempTrack);
    }
    this.showToast(`Воспроизведение: ${item.track_title}`);
  }

  formatHistoryDate(timestampSecs: number): string {
    if (!timestampSecs) return '';
    const date = new Date(timestampSecs * 1000);
    const now = new Date();
    const diffMs = now.getTime() - date.getTime();
    const diffMins = Math.floor(diffMs / 60000);

    if (diffMins < 1) return 'Только что';
    if (diffMins < 60) return `${diffMins} мин назад`;
    
    const diffHours = Math.floor(diffMins / 60);
    if (diffHours < 24) return `${diffHours} ч назад`;

    return date.toLocaleDateString('ru-RU', { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' });
  }

  setView(view: 'all' | 'favorites' | 'uploads' | 'streams' | 'playlist' | 'offline', playlistId?: string) {
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

  toggleFavorite(track: Track, event?: Event) {
    if (event) event.stopPropagation();
    this.libraryService.toggleFavorite(track.id, track);
    if (!track.isFavorite) {
      this.recService.recordTrackLike(track);
      this.showToast('Добавлено в избранное');
    } else {
      this.showToast('Удалено из избранного');
    }
  }

  addToQueue(track: Track, event?: Event) {
    if (event) event.stopPropagation();
    this.audioService.addToQueue(track);
    this.showToast('Добавлено в очередь');
  }

  openAddToPlaylistModal(track: Track, event?: Event) {
    if (event) event.stopPropagation();
    this.targetTrackForPlaylist.set(track);
    this.isAddToPlaylistModalOpen.set(true);
  }

  toggleTrackInPlaylistFromModal(playlist: Playlist) {
    const track = this.targetTrackForPlaylist();
    if (!track) return;
    this.libraryService.addTrackToLibrary({ ...track, playlistOnly: false });
    const isAdded = this.libraryService.toggleTrackInPlaylist(playlist.id, track.id);
    this.showToast(
      isAdded
        ? `Трек добавлен в "${playlist.title}"`
        : `Трек убран из "${playlist.title}"`
    );
  }

  deleteTrack(track: Track, event?: Event) {
    if (event) event.stopPropagation();

    if (this.activeTab() === 'playlist' && this.currentPlaylist()) {
      const pl = this.currentPlaylist()!;
      this.libraryService.removeTrackFromPlaylist(pl.id, track.id);
      this.showToast(`Трек убран из плейлиста "${pl.title}"`);
    } else {
      this.libraryService.deleteTrack(track.id);
      this.showToast(`Трек "${track.title}" удален`);
    }
  }

  async refreshLibrary() {
    this.isRefreshingLibrary.set(true);
    try {
      await this.libraryService.syncWithBackendOnStartup();
      this.showToast('Медиатека обновлена');
    } catch {
      this.showToast('Не удалось обновить медиатеку');
    } finally {
      this.isRefreshingLibrary.set(false);
    }
  }

  clearAllLibraryTracks() {
    if (confirm('Вы действительно хотите удалить все треки из медиатеки?')) {
      this.libraryService.clearAllTracks();
      this.showToast('Медиатека очищена');
    }
  }

  async toggleOfflineTrack(track: Track, event?: Event) {
    if (event) event.stopPropagation();
    if (track.isLiveStream) {
      this.showToast('Прямой эфир нельзя сохранить оффлайн');
      return;
    }

    if (this.offlineService.isTrackOffline(track.id)) {
      await this.offlineService.removeTrackOffline(track.id);
      this.libraryService.updateTrackOfflineStatus(track.id, false);
      this.showToast('Трек удалён из оффлайн-хранилища');
    } else {
      this.showToast('Загрузка трека в кэш...');
      const ok = await this.offlineService.saveTrackOffline(track);
      if (ok) {
        this.libraryService.addTrackToLibrary(track);
        this.libraryService.updateTrackOfflineStatus(track.id, true);
        this.showToast('Трек сохранён для оффлайн-прослушивания!');
      } else {
        this.showToast('Ошибка при загрузке трека');
      }
    }
  }

  async toggleSmartMix(mood: MixMood = 'all') {
    const isCurrentlyActive = this.recService.isMixActive();
    const currentMood = this.recService.currentMood();

    if (isCurrentlyActive && currentMood === mood) {
      this.audioService.togglePlay();
      return;
    }

    if (isCurrentlyActive && currentMood !== mood) {
      this.audioService.setMixMood(mood);
      const moodNames: Record<MixMood, string> = {
        all: 'Все стили',
        energetic: 'Бодрый вайб',
        chill: 'Спокойный чилл',
        favorites: 'Только любимое',
      };
      this.showToast(`Режим волны: ${moodNames[mood]}`);
      return;
    }

    const localCandidates = this.recService.getAllLocalCandidates();
    if (localCandidates.length === 0) {
      this.isQuickStartMixModalOpen.set(true);
      return;
    }

    this.showToast('Запуск Моей Волны...');
    const ok = await this.audioService.startSmartMix(mood);
    if (!ok) {
      this.isQuickStartMixModalOpen.set(true);
    }
  }

  async selectQuickStartVibe(vibe: 'phonk' | 'hiphop' | 'rock' | 'lofi' | 'pop' | 'indie') {
    this.recService.setQuickStartVibe(vibe);
    this.isQuickStartMixModalOpen.set(false);
    this.showToast('Подбираем треки под выбранный стиль...');
    const ok = await this.audioService.startSmartMix('all');
    if (ok) {
      this.showToast('Волна запущена!');
    }
  }

  openMixSettings() {
    this.isMixSettingsModalOpen.set(true);
  }

  closeMixSettings() {
    this.isMixSettingsModalOpen.set(false);
  }

  updateMixMood(mood: MixMood) {
    this.libraryService.setMixConfig({ mood });
    if (this.recService.isMixActive()) {
      this.audioService.setMixMood(mood);
    }
  }

  updateMixSource(source: MixSource) {
    this.libraryService.setMixConfig({ source });
    if (this.recService.isMixActive()) {
      this.audioService.setMixMood(this.recService.currentMood());
    }
  }

  updateMixLanguage(language: MixLanguage) {
    this.libraryService.setMixConfig({ language });
    if (this.recService.isMixActive()) {
      this.audioService.setMixMood(this.recService.currentMood());
    }
  }

  resetMixConfig() {
    this.libraryService.setMixConfig(DEFAULT_MIX_CONFIG);
    if (this.recService.isMixActive()) {
      this.audioService.setMixMood('all');
    }
    this.showToast('Параметры волны сброшены по умолчанию');
  }

  dislikeCurrentTrack() {
    const cur = this.audioService.currentTrack();
    if (!cur) return;
    this.audioService.dislikeCurrentTrack();
    this.showToast(`Трек "${cur.title}" скрыт и не будет звучать`);
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
    this.offlineService.saveTrackOffline(track);
  }

  addOnlineTrackToLib(track: Track) {
    this.libraryService.addTrackToLibrary(track);
    this.audioService.playTrack(track, this.libraryService.tracks());
    this.showToast(`Трек "${track.title}" добавлен и воспроизводится`);
    this.offlineService.saveTrackOffline(track);
  }

  async extractYouTubeUrl() {
    const url = this.youtubeUrlInput().trim();
    if (!url) return;

    this.isExtractingUrl.set(true);
    this.extractError.set(null);
    this.extractedResult.set(null);

    try {
      const res = await this.libraryService.extractFromUrl(url);
      if ((!res.tracks || res.tracks.length === 0) && !res.mainVideo) {
        this.extractError.set('Не удалось извлечь аудио по этой ссылке. Проверьте URL.');
      } else {
        this.extractedResult.set(res);
        if (res.mainVideo && (res.isRadioMix || res.mainVideo.duration > 600 || res.tracks.length <= 1)) {
          this.extractMode.set('single');
        } else {
          this.extractMode.set('playlist');
        }
        this.selectedExtractedTrackIds.set(new Set(res.tracks.map((t) => t.id)));
      }
    } catch {
      this.extractError.set('Ошибка соединения с бэкендом. Убедитесь, что бэкенд запущен.');
    } finally {
      this.isExtractingUrl.set(false);
    }
  }

  addExtractedSingleTrack(playNow = true) {
    const data = this.extractedResult();
    const track = data?.mainVideo || data?.tracks[0];
    if (!track) return;

    this.libraryService.addTrackToLibrary(track);
    this.audioService.playTrack(track, this.libraryService.tracks());
    this.showToast(`Воспроизведение: ${track.title}`);

    // Сохраняем в оффлайн кэш
    this.offlineService.saveTrackOffline(track);

    this.isAddModalOpen.set(false);
    this.youtubeUrlInput.set('');
    this.extractedResult.set(null);
  }

  toggleSelectExtractedTrack(trackId: string) {
    this.selectedExtractedTrackIds.update((set) => {
      const next = new Set(set);
      if (next.has(trackId)) {
        next.delete(trackId);
      } else {
        next.add(trackId);
      }
      return next;
    });
  }

  toggleSelectAllExtractedTracks() {
    const data = this.extractedResult();
    if (!data) return;
    const allIds = data.tracks.map((t) => t.id);
    const current = this.selectedExtractedTrackIds();
    if (current.size === allIds.length) {
      this.selectedExtractedTrackIds.set(new Set());
    } else {
      this.selectedExtractedTrackIds.set(new Set(allIds));
    }
  }

  importAllExtractedTracks(onlySelected = false) {
    const data = this.extractedResult();
    if (!data || data.tracks.length === 0) return;

    let tracksToImport = data.tracks;
    if (onlySelected) {
      const selected = this.selectedExtractedTrackIds();
      tracksToImport = data.tracks.filter((t) => selected.has(t.id));
    }

    if (tracksToImport.length === 0) {
      this.showToast('Выберите хотя бы один трек для импорта');
      return;
    }

    const title = data.playlistTitle || data.mainVideo?.title || 'YouTube Плейлист';
    const createdPl = this.libraryService.importPlaylist(title, tracksToImport);
    this.showToast(`Создан плейлист "${title}" (${tracksToImport.length} треков)`);
    this.isAddModalOpen.set(false);
    this.youtubeUrlInput.set('');
    this.extractedResult.set(null);
    this.setView('playlist', createdPl.id);
  }

  addExtractedTracksToLibraryOnly() {
    const data = this.extractedResult();
    if (!data || data.tracks.length === 0) return;

    const selected = this.selectedExtractedTrackIds();
    const tracksToAdd = selected.size > 0 ? data.tracks.filter((t) => selected.has(t.id)) : data.tracks;

    this.libraryService.addMultipleTracks(tracksToAdd);
    this.showToast(`Добавлено ${tracksToAdd.length} треков в медиатеку`);
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
    const title = this.inputTitle().trim();
    const artist = this.inputArtist().trim();

    if (!url && !title) return;

    this.isUrlValidating.set(true);
    try {
      let track: Track;
      if (!url && title) {
        track = {
          id: 'manual-' + Date.now() + '-' + Math.floor(Math.random() * 1000),
          title,
          artist: artist || 'Разные исполнители',
          album: 'Recro Music',
          duration: 0,
          audioUrl: `/api/stream?title=${encodeURIComponent(title)}&artist=${encodeURIComponent(artist)}`,
          genre: this.inputGenre() || 'Music',
          format: 'mp3',
          bitrate: '192 kbps',
          plays: 0,
          isFavorite: false,
          addedAt: new Date().toISOString().split('T')[0],
        };
        this.libraryService.addTrackToLibrary(track);
      } else {
        track = await this.libraryService.addStreamTrack(
          url,
          title || undefined,
          artist || undefined,
          this.inputGenre() || undefined,
          this.isLiveStreamCheckbox()
        );
      }

      this.showToast(`Трек "${track.title}" добавлен`);
      this.isAddModalOpen.set(false);
      this.inputUrl.set('');
      this.inputTitle.set('');
      this.inputArtist.set('');

      this.audioService.playTrack(track, this.libraryService.tracks());
      this.offlineService.saveTrackOffline(track);
    } catch {
      this.showToast('Ошибка при добавлении трека');
    } finally {
      this.isUrlValidating.set(false);
    }
  }

  addManualFromSearch() {
    const q = this.modalSearchInput().trim();
    if (!q) return;

    let title = q;
    let artist = 'Разные исполнители';
    if (q.includes(' - ')) {
      const parts = q.split(' - ');
      artist = parts[0].trim();
      title = parts.slice(1).join(' - ').trim();
    }

    const track: Track = {
      id: 'manual-' + Date.now() + '-' + Math.floor(Math.random() * 1000),
      title,
      artist,
      album: 'Recro Music',
      duration: 0,
      audioUrl: `/api/stream?title=${encodeURIComponent(title)}&artist=${encodeURIComponent(artist)}`,
      genre: 'Music',
      format: 'mp3',
      bitrate: '192 kbps',
      plays: 0,
      isFavorite: false,
      addedAt: new Date().toISOString().split('T')[0],
    };

    this.libraryService.addTrackToLibrary(track);
    this.audioService.playTrack(track, this.libraryService.tracks());
    this.offlineService.saveTrackOffline(track);
    this.showToast(`Трек "${track.title}" добавлен в медиатеку`);
    this.isAddModalOpen.set(false);
    this.modalSearchInput.set('');
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
