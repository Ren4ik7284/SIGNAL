import { Injectable, signal, computed, inject } from '@angular/core';
import { Track } from '../models/track.model';
import { LibraryService } from './library.service';
import { OfflineService } from './offline.service';
import { RecommendationService, MixMood } from './recommendation.service';

@Injectable({
  providedIn: 'root',
})
export class AudioService {
  private libraryService = inject(LibraryService);
  private offlineService = inject(OfflineService);
  readonly recService = inject(RecommendationService);
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
  private hasRecordedCompletion = false;
  private isReplenishingQueue = false;
  private consecutiveErrorCount = 0;
  private errorTimeoutId: any = null;

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
      const isMobile = typeof navigator !== 'undefined' && /Android|iPhone|iPad|iPod/i.test(navigator.userAgent);
      if (!isMobile) {
        this.audio.crossOrigin = 'anonymous';
      }
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

      // Record track completion at 80% of playback
      const cur = this.currentTrack();
      if (cur && total > 10 && actual >= total * 0.8 && !this.hasRecordedCompletion) {
        this.hasRecordedCompletion = true;
        this.recService.recordTrackCompletion(cur);
      }

      // Proactively ensure smart queue before current track ends
      if (this.recService.isMixActive() && total > 15 && actual >= total - 12) {
        this.ensureSmartQueue();
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
      this.consecutiveErrorCount = 0;
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

    this.audio.addEventListener('error', (e) => {
      console.warn('[AudioService] Audio element error:', e);
      this.handlePlaybackFailure('native_error');
    });
  }

  private handlePlaybackFailure(source: string) {
    console.warn(`[AudioService] Playback failure from ${source}`);
    this.isPlaying.set(false);
    this.updateMediaSessionPlaybackState('paused');

    if (this.errorTimeoutId) {
      clearTimeout(this.errorTimeoutId);
      this.errorTimeoutId = null;
    }

    // NEVER auto-skip or touch dislikes when user is playing their own library tracks
    if (!this.recService.isMixActive()) {
      this.consecutiveErrorCount = 0;
      return;
    }

    this.consecutiveErrorCount++;
    // In wave mode, max 2 skips with calm delay to avoid skipping loop
    if (this.consecutiveErrorCount <= 2) {
      this.errorTimeoutId = setTimeout(() => {
        if (this.recService.isMixActive()) {
          this.next();
        }
      }, 1500);
    } else {
      this.recService.isMixActive.set(false);
      this.consecutiveErrorCount = 0;
    }
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
        title: track.title || 'Recro Track',
        artist: track.artist || 'Recro',
        album: track.album || 'Recro Stream',
        artwork: artwork,
      });
    } catch {
      try {
        navigator.mediaSession.metadata = new MediaMetadata({
          title: track.title || 'Recro Track',
          artist: track.artist || 'Recro',
          album: 'Recro Stream',
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

  async playTrack(track: Track, newQueue?: Track[], fromMix: boolean = false) {
    if (this.errorTimeoutId) {
      clearTimeout(this.errorTimeoutId);
      this.errorTimeoutId = null;
    }

    if (!fromMix) {
      this.recService.isMixActive.set(false);
      this.consecutiveErrorCount = 0;
    }

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

    const isFav = this.libraryService.isTrackFavorite(track);
    this.currentTrack.set({ ...track, isFavorite: isFav });
    this.streamSeekOffset.set(0);
    this.currentTime.set(0);
    this.hasRecordedCompletion = false;

    const initialDuration = track.duration && track.duration > 0 ? track.duration : 0;
    this.duration.set(initialDuration);

    this.updateMediaSessionMetadata(track);
    this.updateMediaSessionPlaybackState('playing');

    let playUrl = track.audioUrl;
    if (this.offlineService.isTrackOffline(track.id)) {
      const offlineBlobUrl = await this.offlineService.getOfflineBlobUrl(track.id);
      if (offlineBlobUrl) {
        playUrl = offlineBlobUrl;
      }
    }

    const activeBase = this.libraryService.getBackendUrl();

    if (!playUrl.startsWith('blob:')) {
      if (playUrl.startsWith('/api/stream')) {
        playUrl = `${activeBase}${playUrl}`;
      } else if (playUrl.includes('/api/stream')) {
        const streamIdx = playUrl.indexOf('/api/stream');
        playUrl = `${activeBase}${playUrl.slice(streamIdx)}`;
      } else if (
        playUrl.includes('youtube.com') ||
        playUrl.includes('youtu.be') ||
        playUrl.includes('soundcloud.com')
      ) {
        playUrl = `${activeBase}/api/stream?url=${encodeURIComponent(playUrl)}`;
      } else if (
        typeof window !== 'undefined' &&
        window.location.protocol === 'https:' &&
        playUrl.startsWith('http://')
      ) {
        playUrl = `${activeBase}/api/stream?url=${encodeURIComponent(playUrl)}`;
      }

      if (playUrl.includes('/api/stream')) {
        if (!playUrl.includes('title=') && track.title) {
          const glue = playUrl.includes('?') ? '&' : '?';
          playUrl = `${playUrl}${glue}title=${encodeURIComponent(track.title)}`;
        }
        if (!playUrl.includes('artist=') && track.artist) {
          const glue = playUrl.includes('?') ? '&' : '?';
          playUrl = `${playUrl}${glue}artist=${encodeURIComponent(track.artist)}`;
        }
      }
    }

    const isMobile = typeof navigator !== 'undefined' && /Android|iPhone|iPad|iPod/i.test(navigator.userAgent);
    if (playUrl.startsWith('blob:') || isMobile) {
      this.audio.removeAttribute('crossorigin');
    } else {
      this.audio.crossOrigin = 'anonymous';
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
        this.handlePlaybackFailure('play_rejection');
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

  stopPlayback() {
    try {
      this.audio.pause();
      this.audio.src = '';
    } catch {}
    this.isPlaying.set(false);
    this.currentTrack.set(null);
    this.updateMediaSessionPlaybackState('none');
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

    const streamIdx = track.audioUrl.indexOf('/api/stream');
    const streamPath = streamIdx !== -1 ? track.audioUrl.slice(streamIdx) : track.audioUrl;
    let baseStreamUrl = `${this.libraryService.getBackendUrl()}${streamPath}`.split('&ss=')[0];
    if (!baseStreamUrl.includes('title=')) {
      const glue = baseStreamUrl.includes('?') ? '&' : '?';
      baseStreamUrl = `${baseStreamUrl}${glue}title=${encodeURIComponent(track.title || '')}&artist=${encodeURIComponent(track.artist || '')}`;
    }
    const ssParam = clamped > 0 ? `&ss=${Math.round(clamped)}` : '';
    const newUrl = `${baseStreamUrl}${ssParam}`;

    this.streamSeekOffset.set(clamped);
    this.currentTime.set(clamped);

    const isMobile = typeof navigator !== 'undefined' && /Android|iPhone|iPad|iPod/i.test(navigator.userAgent);
    if (newUrl.startsWith('blob:') || isMobile) {
      this.audio.removeAttribute('crossorigin');
    } else {
      this.audio.crossOrigin = 'anonymous';
    }

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

    const cur = this.currentTrack();
    if (cur && this.currentTime() < 15 && !this.isLiveStream()) {
      this.recService.recordTrackSkip(cur);
    }

    let nextIdx = this.queueIndex() + 1;
    if (this.isShuffle()) {
      nextIdx = Math.floor(Math.random() * q.length);
    }

    if (nextIdx >= q.length) {
      if (this.recService.isMixActive()) {
        await this.ensureSmartQueue();
        const updatedQ = this.queue();
        if (nextIdx >= updatedQ.length) {
          if (this.repeatMode() === 'all') {
            nextIdx = 0;
          } else {
            return;
          }
        }
      } else if (this.repeatMode() === 'all') {
        nextIdx = 0;
      } else {
        return;
      }
    }

    // Micro-fade before changing track to eliminate clicks
    await this.applyFadeOut(0.18);

    this.queueIndex.set(nextIdx);
    this.playTrack(this.queue()[nextIdx], undefined, this.recService.isMixActive());

    if (this.recService.isMixActive()) {
      this.ensureSmartQueue();
    }
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
    this.playTrack(q[prevIdx], undefined, this.recService.isMixActive());
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

  async ensureSmartQueue() {
    if (!this.recService.isMixActive() || this.isReplenishingQueue) return;
    const q = this.queue();
    const idx = this.queueIndex();
    if (idx < q.length - 2) return;

    this.isReplenishingQueue = true;
    try {
      const existingIds = new Set(q.map((t) => t.id));
      const nextCandidates = this.recService.pickNextTracks(2, existingIds);

      // Discovery rate according to source configuration
      const source = this.recService.mixConfig().source;
      const discoveryChance = source === 'library_only' ? 0 : (source === 'discovery_heavy' ? 0.6 : 0.3);

      if (discoveryChance > 0 && (nextCandidates.length < 2 || Math.random() < discoveryChance)) {
        const discovery = await this.recService.fetchOnlineDiscoveryTracks(2, existingIds);
        for (const d of discovery) {
          nextCandidates.push(d);
          existingIds.add(d.id);
        }
      }

      if (nextCandidates.length > 0) {
        this.queue.update((curQ) => [...curQ, ...nextCandidates]);
      }
    } finally {
      this.isReplenishingQueue = false;
    }
  }

  async startSmartMix(mood: MixMood = 'all'): Promise<boolean> {
    this.recService.isMixActive.set(true);
    this.recService.setMixMood(mood);

    const candidates = this.recService.pickNextTracks(5);
    if (candidates.length === 0) {
      const discovery = await this.recService.fetchOnlineDiscoveryTracks(5);
      if (discovery.length === 0) {
        this.recService.isMixActive.set(false);
        return false;
      }
      this.playTrack(discovery[0], discovery, true);
      return true;
    }

    this.playTrack(candidates[0], candidates, true);
    this.ensureSmartQueue();
    return true;
  }

  stopSmartMix() {
    this.recService.isMixActive.set(false);
  }

  setMixMood(mood: MixMood) {
    this.recService.setMixMood(mood);
    if (this.recService.isMixActive()) {
      const q = this.queue();
      const idx = this.queueIndex();
      const played = q.slice(0, idx + 1);
      const newNext = this.recService.pickNextTracks(4, new Set(played.map((t) => t.id)));
      this.queue.set([...played, ...newNext]);
    }
  }

  updateTrackFavoriteStatus(trackId: string, isFavorite: boolean, trackObj?: Track) {
    const cur = this.currentTrack();
    if (cur) {
      const match = cur.id === trackId || 
        (trackObj && (cur.audioUrl === trackObj.audioUrl || (cur.title.toLowerCase() === trackObj.title.toLowerCase() && cur.artist.toLowerCase() === trackObj.artist.toLowerCase())));
      if (match) {
        this.currentTrack.set({ ...cur, isFavorite });
      }
    }
    this.queue.update((q) =>
      q.map((t) => {
        const match = t.id === trackId || 
          (trackObj && (t.audioUrl === trackObj.audioUrl || (t.title.toLowerCase() === trackObj.title.toLowerCase() && t.artist.toLowerCase() === trackObj.artist.toLowerCase())));
        return match ? { ...t, isFavorite } : t;
      })
    );
  }

  dislikeCurrentTrack() {
    const cur = this.currentTrack();
    if (!cur) return;
    this.recService.dislikeTrack(cur.id);
    this.next();
  }
}
