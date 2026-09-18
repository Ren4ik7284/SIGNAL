import { Injectable, signal, computed, inject } from '@angular/core';
import { Track } from '../models/track.model';
import { LibraryService } from './library.service';

@Injectable({
  providedIn: 'root',
})
export class AudioService {
  private libraryService = inject(LibraryService);
  private audio: HTMLAudioElement;

  readonly currentTrack = signal<Track | null>(null);
  readonly isPlaying = signal<boolean>(false);
  readonly currentTime = signal<number>(0);
  readonly duration = signal<number>(0);
  readonly streamSeekOffset = signal<number>(0);
  readonly volume = signal<number>(0.85);
  readonly isMuted = signal<boolean>(false);
  readonly isShuffle = signal<boolean>(false);
  readonly repeatMode = signal<'off' | 'all' | 'one'>('all');
  readonly queue = signal<Track[]>([]);
  readonly queueIndex = signal<number>(-1);

  private isHandlingEnd = false;

  readonly progressPercent = computed(() => {
    const d = this.duration();
    if (!d || d <= 0 || !isFinite(d)) return 0;
    return Math.min(100, (this.currentTime() / d) * 100);
  });

  readonly isLiveStream = computed(() => {
    const track = this.currentTrack();
    if (!track) return false;
    if (track.isLiveStream) return true;
    const d = this.duration();
    if ((track.duration && track.duration > 0) || (d > 0 && isFinite(d))) {
      return false;
    }
    return true;
  });

  constructor() {
    if (typeof document !== 'undefined') {
      this.audio = document.createElement('audio');
      this.audio.setAttribute('playsinline', 'true');
      this.audio.setAttribute('webkit-playsinline', 'true');
      this.audio.setAttribute('x-webkit-airplay', 'allow');
      this.audio.style.position = 'fixed';
      this.audio.style.width = '0';
      this.audio.style.height = '0';
      this.audio.style.opacity = '0';
      this.audio.style.pointerEvents = 'none';
      this.audio.style.zIndex = '-9999';
      if (document.body) {
        document.body.appendChild(this.audio);
      } else {
        window.addEventListener('DOMContentLoaded', () => {
          document.body.appendChild(this.audio);
        });
      }
    } else {
      this.audio = new Audio();
    }

    this.audio.preload = 'auto';
    this.audio.volume = this.volume();

    this.setupEventListeners();
    this.setupMediaSession();
  }

  private setupEventListeners() {
    this.audio.addEventListener('timeupdate', () => {
      const actual = this.streamSeekOffset() + this.audio.currentTime;
      this.currentTime.set(actual);

      this.updateMediaSessionPosition();

      const total = this.duration();
      if (total > 0 && actual >= total - 0.5 && this.isPlaying()) {
        this.handleTrackEnded();
      }
    });

    this.audio.addEventListener('loadedmetadata', () => {
      const d = this.audio.duration;
      if (d && !isNaN(d) && isFinite(d) && d > 0) {
        this.duration.set(d);
      } else if (this.currentTrack()?.duration && this.currentTrack()!.duration > 0) {
        this.duration.set(this.currentTrack()!.duration);
      }
      this.updateMediaSessionPosition();
    });

    this.audio.addEventListener('durationchange', () => {
      const d = this.audio.duration;
      if (d && !isNaN(d) && isFinite(d) && d > 0) {
        this.duration.set(d);
      } else if (this.currentTrack()?.duration && this.currentTrack()!.duration > 0) {
        this.duration.set(this.currentTrack()!.duration);
      }
      this.updateMediaSessionPosition();
    });

    this.audio.addEventListener('play', () => {
      this.isPlaying.set(true);
      this.updateMediaSessionPlaybackState('playing');
      const cur = this.currentTrack();
      if (cur) this.updateMediaSessionMetadata(cur);
    });

    this.audio.addEventListener('playing', () => {
      this.isPlaying.set(true);
      this.updateMediaSessionPlaybackState('playing');
      const cur = this.currentTrack();
      if (cur) this.updateMediaSessionMetadata(cur);
      this.updateMediaSessionPosition();
    });

    this.audio.addEventListener('pause', () => {
      this.isPlaying.set(false);
      this.updateMediaSessionPlaybackState('paused');
    });

    this.audio.addEventListener('waiting', () => {
      this.updateMediaSessionPlaybackState('paused');
    });

    this.audio.addEventListener('ended', () => {
      this.handleTrackEnded();
    });

    this.audio.addEventListener('error', () => {
      this.isPlaying.set(false);
      this.updateMediaSessionPlaybackState('none');
    });
  }

  private setupMediaSession() {
    if (typeof window === 'undefined' || !('mediaSession' in navigator)) return;

    const setAction = (action: MediaSessionAction, handler: MediaSessionActionHandler | null) => {
      try {
        navigator.mediaSession.setActionHandler(action, handler);
      } catch {}
    };

    setAction('play', () => {
      this.audio.play().catch(() => {});
      this.isPlaying.set(true);
      this.updateMediaSessionPlaybackState('playing');
    });

    setAction('pause', () => {
      this.audio.pause();
      this.isPlaying.set(false);
      this.updateMediaSessionPlaybackState('paused');
    });

    setAction('previoustrack', () => {
      this.prev();
    });

    setAction('nexttrack', () => {
      this.next();
    });

    setAction('seekto', (details) => {
      if (details.seekTime !== undefined && details.seekTime !== null) {
        this.seek(details.seekTime);
      }
    });

    setAction('seekbackward', (details) => {
      this.skipBy(-(details.seekOffset || 10));
    });

    setAction('seekforward', (details) => {
      this.skipBy(details.seekOffset || 10);
    });

    setAction('stop', () => {
      this.audio.pause();
      this.isPlaying.set(false);
      this.updateMediaSessionPlaybackState('none');
    });
  }

  private updateMediaSessionPlaybackState(state: 'playing' | 'paused' | 'none') {
    if (typeof window === 'undefined' || !('mediaSession' in navigator)) return;
    try {
      navigator.mediaSession.playbackState = state;
    } catch {}
  }

  private updateMediaSessionMetadata(track: Track) {
    if (typeof window === 'undefined' || !('mediaSession' in navigator)) return;

    try {
      const origin = window.location.origin;
      const getFullUrl = (url?: string | null) => {
        if (!url) return `${origin}/icons/icon-512.png`;
        if (url.startsWith('http://') || url.startsWith('https://')) return url;
        return `${origin}${url.startsWith('/') ? '' : '/'}${url}`;
      };

      const cover = getFullUrl(track.coverUrl);
      const artwork: MediaImage[] = [
        { src: cover, sizes: '512x512' },
        { src: cover, sizes: '256x256' },
        { src: `${origin}/icons/icon-512.png`, sizes: '512x512', type: 'image/png' },
        { src: `${origin}/icons/icon-192.png`, sizes: '192x192', type: 'image/png' }
      ];

      navigator.mediaSession.metadata = new MediaMetadata({
        title: track.title || 'SIGNAL Track',
        artist: track.artist || 'SIGNAL',
        album: track.album || 'SIGNAL Music',
        artwork: artwork
      });
    } catch {
      try {
        navigator.mediaSession.metadata = new MediaMetadata({
          title: track.title || 'SIGNAL Track',
          artist: track.artist || 'SIGNAL',
          album: 'SIGNAL Music'
        });
      } catch {}
    }
  }

  private updateMediaSessionPosition() {
    if (typeof window === 'undefined' || !('mediaSession' in navigator)) return;
    if (!('setPositionState' in navigator.mediaSession)) return;

    const d = this.duration();
    if (!d || d <= 0 || !isFinite(d) || this.isLiveStream()) {
      try {
        navigator.mediaSession.setPositionState();
      } catch {}
      return;
    }

    try {
      const pos = Math.max(0, Math.min(this.currentTime(), Math.max(0, d - 0.05)));
      navigator.mediaSession.setPositionState({
        duration: d,
        playbackRate: this.audio.playbackRate || 1,
        position: pos
      });
    } catch {}
  }

  playTrack(track: Track, newQueue?: Track[]) {
    if (newQueue && newQueue.length > 0) {
      this.queue.set([...newQueue]);
      const idx = newQueue.findIndex((t) => t.id === track.id);
      this.queueIndex.set(idx >= 0 ? idx : 0);
    } else {
      const currentQueue = this.queue();
      const idx = currentQueue.findIndex((t) => t.id === track.id);
      if (idx === -1) {
        this.queue.set([...currentQueue, track]);
        this.queueIndex.set(this.queue().length - 1);
      } else {
        this.queueIndex.set(idx);
      }
    }

    this.currentTrack.set(track);
    this.streamSeekOffset.set(0);
    this.currentTime.set(0);

    const initialDuration = track.duration && track.duration > 0 ? track.duration : 0;
    this.duration.set(initialDuration);

    this.updateMediaSessionMetadata(track);
    this.updateMediaSessionPlaybackState('playing');

    let playUrl = track.audioUrl;
    if (playUrl.startsWith('/api/stream')) {
      playUrl = `${this.libraryService.getBackendUrl()}${playUrl}`;
    } else if (playUrl.includes('/api/stream')) {
      const activeBase = this.libraryService.getBackendUrl();
      const streamIdx = playUrl.indexOf('/api/stream');
      playUrl = `${activeBase}${playUrl.slice(streamIdx)}`;
    } else if (typeof window !== 'undefined' && window.location.protocol === 'https:' && playUrl.startsWith('http://')) {
      const activeBase = this.libraryService.getBackendUrl();
      playUrl = `${activeBase}/api/stream?url=${encodeURIComponent(playUrl)}`;
    }

    this.audio.src = playUrl;

    this.audio
      .play()
      .then(() => {
        this.isPlaying.set(true);
        this.updateMediaSessionPlaybackState('playing');
        this.updateMediaSessionMetadata(track);
      })
      .catch(() => {
        this.isPlaying.set(false);
        this.updateMediaSessionPlaybackState('paused');
      });
  }

  togglePlay() {
    if (!this.currentTrack()) {
      const q = this.queue();
      if (q.length > 0) {
        this.playTrack(q[0]);
      }
      return;
    }

    const cur = this.currentTrack();
    if (this.audio.paused) {
      if (cur) this.updateMediaSessionMetadata(cur);
      this.updateMediaSessionPlaybackState('playing');
      this.audio
        .play()
        .then(() => {
          this.isPlaying.set(true);
          this.updateMediaSessionPlaybackState('playing');
        })
        .catch(() => {
          this.isPlaying.set(false);
          this.updateMediaSessionPlaybackState('paused');
        });
    } else {
      this.audio.pause();
      this.isPlaying.set(false);
      this.updateMediaSessionPlaybackState('paused');
    }
  }

  seek(seconds: number) {
    if (this.isLiveStream()) return;
    const total = this.duration() || this.currentTrack()?.duration || 0;
    const clamped = Math.max(0, Math.min(seconds, total > 0 ? total : seconds));

    const track = this.currentTrack();
    if (!track) return;

    if (track.audioUrl.includes('/api/stream') || track.audioUrl.startsWith('/api/stream')) {
      const streamIdx = track.audioUrl.indexOf('/api/stream');
      const streamPath = streamIdx !== -1 ? track.audioUrl.slice(streamIdx) : track.audioUrl;
      const baseStreamUrl = `${this.libraryService.getBackendUrl()}${streamPath}`.split('&ss=')[0];
      const ssParam = clamped > 0 ? `&ss=${Math.round(clamped)}` : '';
      const newUrl = `${baseStreamUrl}${ssParam}`;

      this.streamSeekOffset.set(clamped);
      this.currentTime.set(clamped);

      this.audio.src = newUrl;
      this.audio
        .play()
        .then(() => {
          this.isPlaying.set(true);
          this.updateMediaSessionPlaybackState('playing');
        })
        .catch(() => {});
    } else {
      try {
        this.audio.currentTime = clamped;
        this.currentTime.set(clamped);
      } catch {}
    }
    this.updateMediaSessionPosition();
  }

  seekPercent(percent: number) {
    if (this.isLiveStream()) return;
    const total = this.duration() || this.currentTrack()?.duration || 0;
    if (total > 0 && isFinite(total)) {
      const target = (Math.max(0, Math.min(percent, 100)) / 100) * total;
      this.seek(target);
    }
  }

  skipBy(seconds: number) {
    if (this.isLiveStream()) return;
    this.seek(this.currentTime() + seconds);
  }

  next() {
    const q = this.queue();
    if (q.length === 0) return;

    let nextIdx = this.queueIndex() + 1;
    if (this.isShuffle()) {
      nextIdx = Math.floor(Math.random() * q.length);
    }

    if (nextIdx >= q.length) {
      if (this.repeatMode() === 'all') {
        nextIdx = 0;
      } else {
        return;
      }
    }

    this.queueIndex.set(nextIdx);
    this.playTrack(q[nextIdx]);
  }

  prev() {
    if (this.currentTime() > 3) {
      this.seek(0);
      return;
    }

    const q = this.queue();
    if (q.length === 0) return;

    let prevIdx = this.queueIndex() - 1;
    if (prevIdx < 0) {
      prevIdx = q.length - 1;
    }

    this.queueIndex.set(prevIdx);
    this.playTrack(q[prevIdx]);
  }

  private handleTrackEnded() {
    if (this.isHandlingEnd) return;
    this.isHandlingEnd = true;
    setTimeout(() => {
      this.isHandlingEnd = false;
    }, 1200);

    if (this.repeatMode() === 'one') {
      this.seek(0);
      this.audio.play().catch(() => {});
    } else {
      this.next();
    }
  }

  setVolume(vol: number) {
    const clamped = Math.max(0, Math.min(1, vol));
    this.volume.set(clamped);
    this.audio.volume = clamped;
    if (clamped > 0 && this.isMuted()) {
      this.isMuted.set(false);
    }
  }

  toggleMute() {
    if (this.isMuted()) {
      this.audio.volume = this.volume();
      this.isMuted.set(false);
    } else {
      this.audio.volume = 0;
      this.isMuted.set(true);
    }
  }

  toggleShuffle() {
    this.isShuffle.update((v) => !v);
  }

  cycleRepeat() {
    const modes: ('off' | 'all' | 'one')[] = ['all', 'one', 'off'];
    const current = this.repeatMode();
    const next = modes[(modes.indexOf(current) + 1) % modes.length];
    this.repeatMode.set(next);
  }

  addToQueue(track: Track) {
    this.queue.update((q) => [...q, track]);
    if (!this.currentTrack()) {
      this.playTrack(track);
    }
  }

  removeFromQueue(index: number) {
    this.queue.update((q) => q.filter((_, i) => i !== index));
    if (index === this.queueIndex()) {
      this.next();
    } else if (index < this.queueIndex()) {
      this.queueIndex.update((idx) => idx - 1);
    }
  }

  clearQueue() {
    this.queue.set(this.currentTrack() ? [this.currentTrack()!] : []);
    this.queueIndex.set(0);
  }
}
