import { Injectable, signal, computed } from '@angular/core';

export interface UserInfo {
  id: string;
  username: string;
  email?: string;
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

  readonly token = signal<string | null>(null);
  readonly currentUser = signal<UserInfo | null>(null);
  readonly isAuthenticated = computed(() => !!this.token() && !!this.currentUser());
  readonly isAuthLoading = signal<boolean>(false);
  readonly authError = signal<string | null>(null);
  readonly fallbackCode = signal<string | null>(null);

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

  isValidEmail(email: string): boolean {
    const clean = email.trim();
    if (this.hasWhitespace(clean)) return false;
    const re = /^[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)+$/;
    return re.test(clean);
  }

  isValidUsername(username: string): boolean {
    const clean = username.trim();
    if (this.hasWhitespace(clean)) return false;
    if (clean.length < 3 || clean.length > 30) return false;
    return /^[a-zA-Z0-9_-]+$/.test(clean);
  }

  isValidPassword(password: string): boolean {
    const clean = password.trim();
    return clean.length >= 6 && clean.length <= 128;
  }

  async verifyRemoteSession(backendUrl: string) {
    const t = this.token();
    if (!t) return;

    try {
      const res = await fetch(`${backendUrl}/api/auth/me`, {
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
    let base = (backendUrl || '').trim().replace(/\/+$/, '');
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
      this.authError.set('Введите логин или email');
      this.isAuthLoading.set(false);
      return false;
    }

    if (!cleanPass) {
      this.authError.set('Введите пароль');
      this.isAuthLoading.set(false);
      return false;
    }

    if (this.hasWhitespace(cleanLogin)) {
      this.authError.set('Логин или email не должен содержать пробелы внутри');
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
      this.authError.set('Пароль должен содержать не менее 6 символов');
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

  async sendVerificationCode(backendUrl: string, username: string, email: string, password: string): Promise<boolean> {
    this.isAuthLoading.set(true);
    this.authError.set(null);

    const cleanUser = username.trim();
    const cleanEmail = email.trim().toLowerCase();

    if (!this.isValidUsername(cleanUser)) {
      this.authError.set('Имя пользователя должно быть от 3 до 30 символов без пробелов (только латиница, цифры, _ и -)');
      this.isAuthLoading.set(false);
      return false;
    }

    if (!this.isValidEmail(cleanEmail)) {
      this.authError.set('Введите корректный email (например, name@example.com)');
      this.isAuthLoading.set(false);
      return false;
    }

    const cleanPass = password.trim();

    if (!this.isValidPassword(cleanPass)) {
      this.authError.set('Пароль должен содержать от 6 до 128 символов');
      this.isAuthLoading.set(false);
      return false;
    }

    const base = (backendUrl || '').trim().replace(/\/+$/, '') || 'https://signal-audio-backend-production.up.railway.app';

    try {
      const res = await fetch(`${base}/api/auth/send-code`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          username: cleanUser,
          email: cleanEmail,
          password: cleanPass,
        }),
      });

      const data = await res.json().catch(() => null);
      if (!res.ok) {
        this.authError.set(data?.error || 'Ошибка отправки кода');
        return false;
      }

      if (data?.fallback_code) {
        this.fallbackCode.set(data.fallback_code);
      } else {
        this.fallbackCode.set(null);
      }

      return true;
    } catch {
      this.authError.set('Не удалось связаться с сервером');
      return false;
    } finally {
      this.isAuthLoading.set(false);
    }
  }

  async verifyCode(backendUrl: string, email: string, code: string): Promise<boolean> {
    this.isAuthLoading.set(true);
    this.authError.set(null);

    const cleanEmail = email.trim().toLowerCase();
    const cleanCode = code.trim();

    if (cleanCode.length !== 6 || !/^\d{6}$/.test(cleanCode)) {
      this.authError.set('Введите 6-значный цифровой код');
      this.isAuthLoading.set(false);
      return false;
    }

    const base = (backendUrl || '').trim().replace(/\/+$/, '') || 'https://signal-audio-backend-production.up.railway.app';

    try {
      const res = await fetch(`${base}/api/auth/verify-code`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          email: cleanEmail,
          code: cleanCode,
        }),
      });

      const data = await res.json().catch(() => null);
      if (!res.ok) {
        this.authError.set(data?.error || 'Неверный код подтверждения');
        return false;
      }

      this.token.set(data.token);
      this.currentUser.set(data.user);

      try {
        localStorage.setItem(this.TOKEN_STORAGE_KEY, data.token);
        localStorage.setItem(this.USER_STORAGE_KEY, JSON.stringify(data.user));
      } catch {}

      return true;
    } catch {
      this.authError.set('Не удалось завершить подтверждение');
      return false;
    } finally {
      this.isAuthLoading.set(false);
    }
  }

  async resendCode(backendUrl: string, email: string): Promise<boolean> {
    this.isAuthLoading.set(true);
    this.authError.set(null);

    const base = (backendUrl || '').trim().replace(/\/+$/, '') || 'https://signal-audio-backend-production.up.railway.app';

    try {
      const res = await fetch(`${base}/api/auth/resend-code`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email: email.trim().toLowerCase() }),
      });

      const data = await res.json().catch(() => null);
      if (!res.ok) {
        this.authError.set(data?.error || 'Не удалось отправить код повторно');
        return false;
      }

      if (data?.fallback_code) {
        this.fallbackCode.set(data.fallback_code);
      }

      return true;
    } catch {
      this.authError.set('Не удалось связаться с сервером');
      return false;
    } finally {
      this.isAuthLoading.set(false);
    }
  }

  async requestPasswordReset(backendUrl: string, loginOrEmail: string): Promise<boolean> {
    this.isAuthLoading.set(true);
    this.authError.set(null);

    const clean = loginOrEmail.trim();
    if (!clean) {
      this.authError.set('Введите логин или email');
      this.isAuthLoading.set(false);
      return false;
    }

    const base = (backendUrl || '').trim().replace(/\/+$/, '') || 'https://signal-audio-backend-production.up.railway.app';

    try {
      const res = await fetch(`${base}/api/auth/reset-password-code`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ email_or_login: clean }),
      });

      const data = await res.json().catch(() => null);
      if (!res.ok) {
        this.authError.set(data?.error || 'Не удалось запросить сброс пароля');
        return false;
      }

      if (data?.fallback_code) {
        this.fallbackCode.set(data.fallback_code);
      } else {
        this.fallbackCode.set(null);
      }

      return true;
    } catch {
      this.authError.set('Не удалось связаться с сервером');
      return false;
    } finally {
      this.isAuthLoading.set(false);
    }
  }

  async confirmPasswordReset(backendUrl: string, loginOrEmail: string, code: string, newPassword: string): Promise<boolean> {
    this.isAuthLoading.set(true);
    this.authError.set(null);

    const cleanInput = loginOrEmail.trim();
    const cleanCode = code.trim();
    const cleanPass = newPassword.trim();

    if (cleanCode.length !== 6 || !/^\d{6}$/.test(cleanCode)) {
      this.authError.set('Введите 6-значный цифровой код');
      this.isAuthLoading.set(false);
      return false;
    }

    if (cleanPass.length < 6) {
      this.authError.set('Новый пароль должен быть не менее 6 символов');
      this.isAuthLoading.set(false);
      return false;
    }

    const base = (backendUrl || '').trim().replace(/\/+$/, '') || 'https://signal-audio-backend-production.up.railway.app';

    try {
      const res = await fetch(`${base}/api/auth/reset-password`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          email_or_login: cleanInput,
          code: cleanCode,
          new_password: cleanPass,
        }),
      });

      const data = await res.json().catch(() => null);
      if (!res.ok) {
        this.authError.set(data?.error || 'Не удалось сбросить пароль');
        return false;
      }

      this.token.set(data.token);
      this.currentUser.set(data.user);

      try {
        localStorage.setItem(this.TOKEN_STORAGE_KEY, data.token);
        localStorage.setItem(this.USER_STORAGE_KEY, JSON.stringify(data.user));
      } catch {}

      return true;
    } catch {
      this.authError.set('Не удалось связаться с сервером');
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
    this.fallbackCode.set(null);
    try {
      localStorage.removeItem(this.TOKEN_STORAGE_KEY);
      localStorage.removeItem(this.USER_STORAGE_KEY);
    } catch {}
  }
}
