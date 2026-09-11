import { Injectable, signal, computed } from '@angular/core';
import { Track } from '../models/track.model';

@Injectable({
  providedIn: 'root',
})
export class AudioService {
  private audio: HTMLAudioElement;

  // Reactive State Signals
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

  // Computed properties
  readonly progressPercent = computed(() => {
    const d = this.duration();
    if (!d || d <= 0 || !isFinite(d)) return 0;
    return Math.min(100, (this.currentTime() / d) * 100);
  });

  readonly isLiveStream = computed(() => {
    const track = this.currentTrack();
    if (!track) return false;
    // Explicit 24/7 radio stations
    if (track.isLiveStream) return true;
    const d = this.duration();
    // If track or audio element has finite duration, it is a normal seekable track
    if ((track.duration && track.duration > 0) || (d > 0 && isFinite(d))) {
      return false;
    }
    return true;
  });

  constructor() {
    this.audio = new Audio();
    this.audio.preload = 'metadata';
    this.audio.volume = this.volume();

    this.setupEventListeners();
  }

  private setupEventListeners() {
    this.audio.addEventListener('timeupdate', () => {
      const actual = this.streamSeekOffset() + this.audio.currentTime;
      this.currentTime.set(actual);

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
    });

    this.audio.addEventListener('durationchange', () => {
      const d = this.audio.duration;
      if (d && !isNaN(d) && isFinite(d) && d > 0) {
        this.duration.set(d);
      } else if (this.currentTrack()?.duration && this.currentTrack()!.duration > 0) {
        this.duration.set(this.currentTrack()!.duration);
      }
    });

    this.audio.addEventListener('play', () => {
      this.isPlaying.set(true);
    });

    this.audio.addEventListener('pause', () => {
      this.isPlaying.set(false);
    });

    this.audio.addEventListener('ended', () => {
      this.handleTrackEnded();
    });

    this.audio.addEventListener('error', (e) => {
      console.warn('Playback notice:', e);
      this.isPlaying.set(false);
    });
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

    this.audio.src = track.audioUrl;
    this.audio.load();

    this.audio
      .play()
      .then(() => {
        this.isPlaying.set(true);
      })
      .catch((err) => {
        console.warn('Auto-play blocked or network delay:', err);
        this.isPlaying.set(false);
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

    if (this.audio.paused) {
      this.audio.play().then(() => this.isPlaying.set(true)).catch(() => {});
    } else {
      this.audio.pause();
      this.isPlaying.set(false);
    }
  }

  seek(seconds: number) {
    if (this.isLiveStream()) return;
    const total = this.duration() || this.currentTrack()?.duration || 0;
    const clamped = Math.max(0, Math.min(seconds, total > 0 ? total : seconds));

    const track = this.currentTrack();
    if (!track) return;

    // Check if this is our stream endpoint (supports fast server-side &ss= parameter)
    if (track.audioUrl.includes('/api/stream')) {
      const baseUrl = track.audioUrl.split('&ss=')[0];
      const ssParam = clamped > 0 ? `&ss=${Math.round(clamped)}` : '';
      const newUrl = `${baseUrl}${ssParam}`;

      this.streamSeekOffset.set(clamped);
      this.currentTime.set(clamped);

      this.audio.src = newUrl;
      this.audio.load();
      this.audio
        .play()
        .then(() => this.isPlaying.set(true))
        .catch(() => {});
    } else {
      // Local file or standard static stream with byte-range support
      try {
        this.audio.currentTime = clamped;
        this.currentTime.set(clamped);
      } catch (err) {
        console.warn('Seek error:', err);
      }
    }
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
