import { Injectable, signal, computed, inject } from '@angular/core';
import { Track } from '../models/track.model';
import { LibraryService } from './library.service';
import { OfflineService } from './offline.service';

@Injectable({
  providedIn: 'root',
})
export class AudioService {
  private libraryService = inject(LibraryService);
  private offlineService = inject(OfflineService);
  private audio: HTMLAudioElement;

  // Web Audio API Nodes for Normalization & Crossfade & Visualizer
  private audioCtx: AudioContext | null = null;
  private sourceNode: MediaElementAudioSourceNode | null = null;
  private compressorNode: DynamicsCompressorNode | null = null;
  private gainNode: GainNode | null = null;
  private analyserNode: AnalyserNode | null = null;
  private isAudioGraphReady = false;

  readonly isNormalizationEnabled = signal<boolean>(true);
  readonly isCrossfadeEnabled = signal<boolean>(true);
  readonly isVisualizerOpen = signal<boolean>(false);

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
  private isFadingOut = false;

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
      this.audio.crossOrigin = 'anonymous';
      this.audio.preload = 'auto';
      this.audio.style.position = 'fixed';
      this.audio.style.width = '1px';
      this.audio.style.height = '1px';
      this.audio.style.opacity = '0.01';
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
      this.audio.crossOrigin = 'anonymous';
    }

    this.audio.volume = this.volume();

    this.setupEventListeners();
    this.setupMediaSession();
  }

  /**
   * Initializes Web Audio API graph:
   * HTMLAudioElement -> MediaElementSourceNode -> DynamicsCompressorNode (Peak Limiting) -> GainNode (Fade) -> Destination
   */
  private initAudioContext() {
    if (this.isAudioGraphReady || typeof window === 'undefined') return;

    // Mobile browsers (Chrome Android / iOS Safari) kill WebAudio graphs on screen lock.
    // Keeping native HTML5 audio output on mobile guarantees the Lock Screen & Notification widget stays active.
    const isMobile = typeof navigator !== 'undefined' && /Android|iPhone|iPad|iPod/i.test(navigator.userAgent);
    if (isMobile) {
      return;
    }

    try {
      const AudioCtxClass = window.AudioContext || (window as any).webkitAudioContext;
      if (!AudioCtxClass) return;

      this.audioCtx = new AudioCtxClass();
      this.sourceNode = this.audioCtx.createMediaElementSource(this.audio);

      // 1. DynamicsCompressor for peak volume leveling across YouTube, SoundCloud, and Radio
      this.compressorNode = this.audioCtx.createDynamicsCompressor();
      this.compressorNode.threshold.setValueAtTime(-22, this.audioCtx.currentTime);
      this.compressorNode.knee.setValueAtTime(28, this.audioCtx.currentTime);
      this.compressorNode.ratio.setValueAtTime(10, this.audioCtx.currentTime);
      this.compressorNode.attack.setValueAtTime(0.003, this.audioCtx.currentTime);
      this.compressorNode.release.setValueAtTime(0.25, this.audioCtx.currentTime);

      // 2. GainNode for smooth crossfades and click-free track transitions
      this.gainNode = this.audioCtx.createGain();
      this.gainNode.gain.setValueAtTime(1.0, this.audioCtx.currentTime);

      // 3. AnalyserNode for real-time audio visualization
      this.analyserNode = this.audioCtx.createAnalyser();
      this.analyserNode.fftSize = 256;
      this.analyserNode.smoothingTimeConstant = 0.82;

      // Connect graph: Source -> Compressor -> Gain -> Analyser -> Destination
      this.sourceNode.connect(this.compressorNode);
      this.compressorNode.connect(this.gainNode);
      this.gainNode.connect(this.analyserNode);
      this.analyserNode.connect(this.audioCtx.destination);

      this.isAudioGraphReady = true;
    } catch (err) {
      console.warn('[AudioService] Web Audio API graph not available, using standard HTML5 Audio:', err);
    }
  }

  ensureAudioContext() {
    this.initAudioContext();
    if (this.audioCtx && this.audioCtx.state === 'suspended') {
      this.audioCtx.resume().catch(() => {});
    }
  }

  toggleVisualizer() {
    this.ensureAudioContext();
    this.isVisualizerOpen.update((v) => !v);
  }

  openVisualizer() {
    this.ensureAudioContext();
    this.isVisualizerOpen.set(true);
  }

  closeVisualizer() {
    this.isVisualizerOpen.set(false);
  }

  getAudioFrequencyData(array: Uint8Array): boolean {
    if (!this.analyserNode) return false;
    try {
      this.analyserNode.getByteFrequencyData(array as any);
      return true;
    } catch {
      return false;
    }
  }

  getAudioTimeDomainData(array: Uint8Array): boolean {
    if (!this.analyserNode) return false;
    try {
      this.analyserNode.getByteTimeDomainData(array as any);
      return true;
    } catch {
      return false;
    }
  }

  private applyFadeIn(durationSec = 1.6) {
    if (!this.gainNode || !this.audioCtx || !this.isCrossfadeEnabled()) return;
    try {
      const now = this.audioCtx.currentTime;
      this.gainNode.gain.cancelScheduledValues(now);
      this.gainNode.gain.setValueAtTime(0.02, now);
      this.gainNode.gain.linearRampToValueAtTime(1.0, now + durationSec);
    } catch {}
  }

  private applyFadeOut(durationSec = 2.0): Promise<void> {
    return new Promise((resolve) => {
      if (!this.gainNode || !this.audioCtx || !this.isCrossfadeEnabled()) {
        resolve();
        return;
      }
      try {
        const now = this.audioCtx.currentTime;
        this.gainNode.gain.cancelScheduledValues(now);
        this.gainNode.gain.setValueAtTime(this.gainNode.gain.value, now);
        this.gainNode.gain.linearRampToValueAtTime(0.02, now + durationSec);
        setTimeout(resolve, durationSec * 1000);
      } catch {
        resolve();
      }
    });
  }

  toggleNormalization() {
    this.isNormalizationEnabled.update((v) => !v);
    if (!this.sourceNode || !this.gainNode || !this.audioCtx) return;

    try {
      this.sourceNode.disconnect();
      if (this.compressorNode) this.compressorNode.disconnect();

      if (this.isNormalizationEnabled() && this.compressorNode) {
        this.sourceNode.connect(this.compressorNode);
        this.compressorNode.connect(this.gainNode);
      } else {
        this.sourceNode.connect(this.gainNode);
      }
    } catch {}
  }

  toggleCrossfade() {
    this.isCrossfadeEnabled.update((v) => !v);
  }

  private setupEventListeners() {
    this.audio.addEventListener('timeupdate', () => {
      const actual = this.streamSeekOffset() + this.audio.currentTime;
      this.currentTime.set(actual);

      const total = this.duration();
      // Smooth fade-out 2.5s before end of track
      if (total > 3 && actual >= total - 2.5 && !this.isFadingOut && !this.isHandlingEnd && this.isCrossfadeEnabled()) {
        this.isFadingOut = true;
        this.applyFadeOut(2.2);
      }

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
      this.updateMediaSessionPosition();
    });

    this.audio.addEventListener('playing', () => {
      this.isPlaying.set(true);
      this.updateMediaSessionPlaybackState('playing');
      const cur = this.currentTrack();
      if (cur) this.updateMediaSessionMetadata(cur);
      this.updateMediaSessionPosition();
    });

    this.audio.addEventListener('pause', () => {
      // If we are simply rebuffering or handling end, ignore pause event
      if (!this.isHandlingEnd) {
        this.isPlaying.set(false);
        this.updateMediaSessionPlaybackState('paused');
        this.updateMediaSessionPosition();
      }
    });

    this.audio.addEventListener('waiting', () => {
      // NOTE: Do NOT set playbackState = 'paused' on waiting!
      // Mobile Chrome drops the notification shade card if set to paused while buffering.
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
      this.togglePlay();
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
        { src: cover, sizes: '512x512', type: 'image/jpeg' },
        { src: cover, sizes: '256x256', type: 'image/jpeg' },
        { src: `${origin}/icons/icon-512.png`, sizes: '512x512', type: 'image/png' },
        { src: `${origin}/icons/icon-192.png`, sizes: '192x192', type: 'image/png' },
      ];

      navigator.mediaSession.metadata = new MediaMetadata({
        title: track.title || 'SIGNAL Track',
        artist: track.artist || 'SIGNAL',
        album: track.album || 'SIGNAL Stream',
        artwork: artwork,
      });
    } catch {
      try {
        navigator.mediaSession.metadata = new MediaMetadata({
          title: track.title || 'SIGNAL Track',
          artist: track.artist || 'SIGNAL',
          album: 'SIGNAL Stream',
        });
      } catch {}
    }
  }

  private updateMediaSessionPosition() {
    if (typeof window === 'undefined' || !('mediaSession' in navigator)) return;
    if (!('setPositionState' in navigator.mediaSession)) return;

    const d = this.duration();
    if (!d || d <= 0 || !isFinite(d) || this.isLiveStream()) {
      return;
    }

    try {
      const pos = Math.max(0, Math.min(this.currentTime(), Math.max(0, d - 0.05)));
      navigator.mediaSession.setPositionState({
        duration: d,
        playbackRate: this.audio.playbackRate || 1,
        position: pos,
      });
    } catch {}
  }

  async playTrack(track: Track, newQueue?: Track[]) {
    // 1. Initialize Web Audio API on user gesture
    this.initAudioContext();
    if (this.audioCtx && this.audioCtx.state === 'suspended') {
      try {
        await this.audioCtx.resume();
      } catch {}
    }

    this.isFadingOut = false;

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

    // 2. Check if track is cached offline in Cache API
    let playUrl = track.audioUrl;
    const offlineBlobUrl = await this.offlineService.getOfflineBlobUrl(track.id);
    if (offlineBlobUrl) {
      playUrl = offlineBlobUrl;
    } else if (playUrl.startsWith('/api/stream')) {
      playUrl = `${this.libraryService.getBackendUrl()}${playUrl}`;
    } else if (playUrl.includes('/api/stream')) {
      const activeBase = this.libraryService.getBackendUrl();
      const streamIdx = playUrl.indexOf('/api/stream');
      playUrl = `${activeBase}${playUrl.slice(streamIdx)}`;
    } else if (
      typeof window !== 'undefined' &&
      window.location.protocol === 'https:' &&
      playUrl.startsWith('http://')
    ) {
      const activeBase = this.libraryService.getBackendUrl();
      playUrl = `${activeBase}/api/stream?url=${encodeURIComponent(playUrl)}`;
    }

    this.audio.src = playUrl;

    // Apply smooth fade in
    this.applyFadeIn(1.5);

    this.audio
      .play()
      .then(() => {
        this.isPlaying.set(true);
        this.updateMediaSessionPlaybackState('playing');
        this.updateMediaSessionMetadata(track);
        this.updateMediaSessionPosition();
        this.libraryService.recordHistoryPlay(track);
      })
      .catch((err) => {
        console.warn('[AudioService] play() error:', err);
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

    this.ensureAudioContext();

    const cur = this.currentTrack();
    if (this.audio.paused) {
      if (cur) this.updateMediaSessionMetadata(cur);
      this.updateMediaSessionPlaybackState('playing');
      this.applyFadeIn(0.5);
      this.audio
        .play()
        .then(() => {
          this.isPlaying.set(true);
          this.updateMediaSessionPlaybackState('playing');
          this.updateMediaSessionPosition();
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

    // If audio is playing from a local blob URL or direct static audio, seek natively without restarting stream
    if (this.audio.src.startsWith('blob:') || !this.audio.src.includes('/api/stream')) {
      try {
        this.audio.currentTime = clamped;
        this.currentTime.set(clamped);
        this.updateMediaSessionPosition();
      } catch {}
      return;
    }

    // Otherwise proxy seek via &ss= query parameter
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
        this.updateMediaSessionPosition();
      })
      .catch(() => {});
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

  async next() {
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

    // Micro-fade before changing track to eliminate clicks
    await this.applyFadeOut(0.18);

    this.queueIndex.set(nextIdx);
    this.playTrack(q[nextIdx]);
  }

  async prev() {
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

    // Micro-fade before changing track
    await this.applyFadeOut(0.18);

    this.queueIndex.set(prevIdx);
    this.playTrack(q[prevIdx]);
  }

  private handleTrackEnded() {
    if (this.isHandlingEnd) return;
    this.isHandlingEnd = true;
    setTimeout(() => {
      this.isHandlingEnd = false;
      this.isFadingOut = false;
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
