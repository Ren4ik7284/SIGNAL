import { Injectable, signal, computed } from '@angular/core';

export interface UserInfo {
  id: string;
  username: string;
  email?: string;
  avatar_url?: string;
}

export interface AuthConfigResponse {
  google_client_id?: string;
  google_auth_enabled: boolean;
}

export interface AuthResponse {
  token: string;
  user: UserInfo;
}

export interface TopTrackItem {
  track_id: string;
  title: string;
  artist: string;
  cover_url?: string;
  plays: number;
}

export interface TopArtistItem {
  artist: string;
  plays: number;
}

export interface TopGenreItem {
  genre: string;
  plays: number;
  percentage: number;
}

export interface MusicPersonality {
  title: string;
  tag: string;
  description: string;
}

export interface WrappedStats {
  username: string;
  total_plays: number;
  total_minutes: number;
  unique_tracks: number;
  unique_artists: number;
  top_tracks: TopTrackItem[];
  top_artists: TopArtistItem[];
  top_genres: TopGenreItem[];
  personality: MusicPersonality;
}

export interface HistoryItem {
  id: number;
  track_id: string;
  track_title: string;
  track_artist: string;
  track_genre?: string;
  cover_url?: string;
  duration?: number;
  played_at: number;
}

@Injectable({
  providedIn: 'root',
})
export class AuthService {
  private readonly TOKEN_STORAGE_KEY = 'signal_auth_jwt_token';
  private readonly USER_STORAGE_KEY = 'signal_auth_user_data';
  private readonly GOOGLE_CLIENT_ID_STORAGE_KEY = 'signal_google_client_id';

  readonly token = signal<string | null>(null);
  readonly currentUser = signal<UserInfo | null>(null);
  readonly isAuthenticated = computed(() => !!this.token() && !!this.currentUser());
  readonly isAuthLoading = signal<boolean>(false);
  readonly authError = signal<string | null>(null);

  readonly googleClientId = signal<string | null>(null);
  readonly isGoogleAuthEnabled = signal<boolean>(false);

  constructor() {
    this.restoreSession();
  }

  private restoreSession() {
    if (typeof localStorage === 'undefined') return;
    try {
      const savedToken = localStorage.getItem(this.TOKEN_STORAGE_KEY);
      const savedUser = localStorage.getItem(this.USER_STORAGE_KEY);
      if (savedToken && savedUser) {
        this.token.set(savedToken);
        this.currentUser.set(JSON.parse(savedUser));
      }
      const savedGoogleId = localStorage.getItem(this.GOOGLE_CLIENT_ID_STORAGE_KEY);
      if (savedGoogleId) {
        this.googleClientId.set(savedGoogleId);
        this.isGoogleAuthEnabled.set(true);
      }
    } catch {
      this.clearSession();
    }
  }

  getAuthHeaders(): Record<string, string> {
    const t = this.token();
    if (!t) return {};
    return {
      Authorization: `Bearer ${t}`,
    };
  }

  hasWhitespace(str: string): boolean {
    return /\s/.test(str.trim());
  }

  isValidUsername(username: string): boolean {
    const clean = username.trim();
    if (this.hasWhitespace(clean)) return false;
    if (clean.length < 3 || clean.length > 30) return false;
    return /^[a-zA-Z0-9_-]+$/.test(clean);
  }

  isValidPassword(password: string): boolean {
    const clean = password.trim();
    return clean.length >= 6 && clean.length <= 72;
  }

  private activeBackendUrl = 'https://signal-audio-backend-production.up.railway.app';

  setActiveBackendUrl(url: string) {
    if (url) {
      this.activeBackendUrl = url.trim().replace(/\/+$/, '');
    }
  }

  async verifyRemoteSession(backendUrl: string) {
    this.setActiveBackendUrl(backendUrl);
    const t = this.token();
    if (!t) return;

    const base = this.getEffectiveBackendUrl(backendUrl);
    try {
      const res = await fetch(`${base}/api/auth/me`, {
        headers: this.getAuthHeaders(),
      });
      if (res.ok) {
        const user: UserInfo = await res.json();
        this.currentUser.set(user);
        try {
          localStorage.setItem(this.USER_STORAGE_KEY, JSON.stringify(user));
        } catch {}
      } else if (res.status === 401) {
        this.clearSession();
      }
    } catch {}
  }

  private getEffectiveBackendUrl(backendUrl?: string): string {
    let base = (backendUrl || this.activeBackendUrl || '').trim().replace(/\/+$/, '');
    const isHttps = typeof window !== 'undefined' && window.location.protocol === 'https:';
    if (isHttps && base.startsWith('http://')) {
      base = 'https://signal-audio-backend-production.up.railway.app';
    }
    return base || 'https://signal-audio-backend-production.up.railway.app';
  }

  async login(backendUrl: string, login: string, password: string): Promise<boolean> {
    this.isAuthLoading.set(true);
    this.authError.set(null);

    const cleanLogin = login.trim();
    const cleanPass = password.trim();

    if (!cleanLogin) {
      this.authError.set('Введите имя пользователя');
      this.isAuthLoading.set(false);
      return false;
    }

    if (!cleanPass) {
      this.authError.set('Введите пароль');
      this.isAuthLoading.set(false);
      return false;
    }

    if (this.hasWhitespace(cleanLogin)) {
      this.authError.set('Имя пользователя не должно содержать пробелы');
      this.isAuthLoading.set(false);
      return false;
    }

    const base = this.getEffectiveBackendUrl(backendUrl);

    try {
      const res = await fetch(`${base}/api/auth/login`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ login: cleanLogin, password: cleanPass }),
      });

      const data = await res.json().catch(() => null);
      if (!res.ok) {
        this.authError.set(data?.error || 'Ошибка входа в систему');
        return false;
      }

      this.token.set(data.token);
      this.currentUser.set(data.user);

      try {
        localStorage.setItem(this.TOKEN_STORAGE_KEY, data.token);
        localStorage.setItem(this.USER_STORAGE_KEY, JSON.stringify(data.user));
      } catch {}

      return true;
    } catch (e: any) {
      console.error('[SIGNAL AUTH LOGIN ERROR]', e);
      this.authError.set('Не удалось подключиться к серверу');
      return false;
    } finally {
      this.isAuthLoading.set(false);
    }
  }

  async register(backendUrl: string, username: string, password: string): Promise<boolean> {
    this.isAuthLoading.set(true);
    this.authError.set(null);

    const cleanUser = username.trim();
    const cleanPass = password.trim();

    if (!cleanUser) {
      this.authError.set('Введите имя пользователя');
      this.isAuthLoading.set(false);
      return false;
    }

    if (this.hasWhitespace(cleanUser)) {
      this.authError.set('Имя пользователя не должно содержать пробелы');
      this.isAuthLoading.set(false);
      return false;
    }

    if (!this.isValidUsername(cleanUser)) {
      this.authError.set('Имя пользователя должно быть от 3 до 30 символов (латиница, цифры, _ и -)');
      this.isAuthLoading.set(false);
      return false;
    }

    if (!this.isValidPassword(cleanPass)) {
      this.authError.set('Пароль должен содержать от 6 до 72 символов');
      this.isAuthLoading.set(false);
      return false;
    }

    const base = this.getEffectiveBackendUrl(backendUrl);

    try {
      const res = await fetch(`${base}/api/auth/register`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ username: cleanUser, password: cleanPass }),
      });

      const data = await res.json().catch(() => null);
      if (!res.ok) {
        this.authError.set(data?.error || 'Ошибка регистрации');
        return false;
      }

      this.token.set(data.token);
      this.currentUser.set(data.user);

      try {
        localStorage.setItem(this.TOKEN_STORAGE_KEY, data.token);
        localStorage.setItem(this.USER_STORAGE_KEY, JSON.stringify(data.user));
      } catch {}

      return true;
    } catch (e: any) {
      console.error('[SIGNAL AUTH REGISTER ERROR]', e);
      this.authError.set('Не удалось подключиться к серверу');
      return false;
    } finally {
      this.isAuthLoading.set(false);
    }
  }

  async fetchAuthConfig(backendUrl: string): Promise<void> {
    const base = this.getEffectiveBackendUrl(backendUrl);
    try {
      const res = await fetch(`${base}/api/auth/config`);
      if (res.ok) {
        const data: AuthConfigResponse = await res.json();
        if (data.google_client_id) {
          this.googleClientId.set(data.google_client_id);
          this.isGoogleAuthEnabled.set(true);
        } else {
          const localId = localStorage.getItem(this.GOOGLE_CLIENT_ID_STORAGE_KEY);
          if (localId) {
            this.googleClientId.set(localId);
            this.isGoogleAuthEnabled.set(true);
          }
        }
      }
    } catch (e) {
      console.warn('[SIGNAL AUTH] Не удалось загрузить конфигурацию аутентификации:', e);
    }
  }

  setCustomGoogleClientId(clientId: string | null) {
    if (clientId && clientId.trim()) {
      const clean = clientId.trim();
      this.googleClientId.set(clean);
      this.isGoogleAuthEnabled.set(true);
      try {
        localStorage.setItem(this.GOOGLE_CLIENT_ID_STORAGE_KEY, clean);
      } catch {}
    } else {
      this.googleClientId.set(null);
      this.isGoogleAuthEnabled.set(false);
      try {
        localStorage.removeItem(this.GOOGLE_CLIENT_ID_STORAGE_KEY);
      } catch {}
    }
  }

  async loginWithGoogle(backendUrl: string, credential: string): Promise<boolean> {
    this.isAuthLoading.set(true);
    this.authError.set(null);

    const base = this.getEffectiveBackendUrl(backendUrl);

    try {
      const res = await fetch(`${base}/api/auth/google`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ credential }),
      });

      const data = await res.json().catch(() => null);
      if (!res.ok) {
        this.authError.set(data?.error || 'Ошибка входа через Google');
        return false;
      }

      this.token.set(data.token);
      this.currentUser.set(data.user);

      try {
        localStorage.setItem(this.TOKEN_STORAGE_KEY, data.token);
        localStorage.setItem(this.USER_STORAGE_KEY, JSON.stringify(data.user));
      } catch {}

      return true;
    } catch (e: any) {
      console.error('[SIGNAL GOOGLE AUTH ERROR]', e);
      this.authError.set('Не удалось подключиться к серверу для авторизации через Google');
      return false;
    } finally {
      this.isAuthLoading.set(false);
    }
  }

  logout() {
    this.clearSession();
  }

  private clearSession() {
    this.token.set(null);
    this.currentUser.set(null);
    try {
      localStorage.removeItem(this.TOKEN_STORAGE_KEY);
      localStorage.removeItem(this.USER_STORAGE_KEY);
    } catch {}
  }
}
