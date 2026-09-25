import { Injectable, signal, computed, inject } from '@angular/core';
import { Track, MixConfig, MixMood, MixSource, MixLanguage } from '../models/track.model';
import { LibraryService } from './library.service';

export type { MixMood };

export interface TasteVector {
  energy: number;     // 0 (ambient/soft) -> 1 (heavy bass/phonk/rock)
  tempo: number;      // 0 (slow <80 BPM) -> 1 (fast >140 BPM)
  acoustic: number;   // 0 (electronic/synthesized) -> 1 (live instruments/acoustic)
  hiphop: number;     // 0..1 weight
  rock: number;       // 0..1 weight
  electronic: number; // 0..1 weight
  pop: number;        // 0..1 weight
  chill: number;      // 0..1 weight
}

const DEFAULT_TASTE_VECTOR: TasteVector = {
  energy: 0.6,
  tempo: 0.55,
  acoustic: 0.35,
  hiphop: 0.5,
  rock: 0.4,
  electronic: 0.5,
  pop: 0.5,
  chill: 0.4,
};

@Injectable({
  providedIn: 'root',
})
export class RecommendationService {
  private readonly libraryService = inject(LibraryService);

  private readonly STORAGE_KEY_TASTE = 'recro_taste_vector_v1';
  private readonly STORAGE_KEY_DISLIKES = 'recro_disliked_tracks_v1';

  readonly isMixActive = signal<boolean>(false);
  readonly mixConfig = this.libraryService.mixConfig;
  readonly currentMood = computed<MixMood>(() => this.libraryService.mixConfig().mood);
  readonly isFetchingDiscovery = signal<boolean>(false);

  // User Taste Vector in reactive state
  readonly tasteVector = signal<TasteVector>(this.loadSavedTasteVector());

  // Recent play timestamps to prevent repetition (Fatigue Penalty)
  // Map of trackId -> epoch timestamp (ms)
  private recentPlays = new Map<string, number>();

  // Disliked tracks (delegated to LibraryService with cloud sync)
  readonly dislikedTrackIds = this.libraryService.dislikedTrackIds;

  constructor() {
    this.cleanupOldPlays();
  }

  setMixMood(mood: MixMood) {
    this.libraryService.setMixConfig({ mood });
  }

  updateConfig(partial: Partial<MixConfig>) {
    this.libraryService.setMixConfig(partial);
  }

  private loadSavedTasteVector(): TasteVector {
    if (typeof localStorage === 'undefined') return { ...DEFAULT_TASTE_VECTOR };
    try {
      const saved = localStorage.getItem(this.STORAGE_KEY_TASTE);
      if (saved) {
        return { ...DEFAULT_TASTE_VECTOR, ...JSON.parse(saved) };
      }
    } catch {}
    return { ...DEFAULT_TASTE_VECTOR };
  }

  private saveTasteVector(vec: TasteVector) {
    if (typeof localStorage === 'undefined') return;
    try {
      localStorage.setItem(this.STORAGE_KEY_TASTE, JSON.stringify(vec));
    } catch {}
  }

  private cleanupOldPlays() {
    const now = Date.now();
    const fourHours = 4 * 60 * 60 * 1000;
    for (const [id, time] of this.recentPlays.entries()) {
      if (now - time > fourHours) {
        this.recentPlays.delete(id);
      }
    }
  }

  /**
   * Generates a normalized 8-dimensional music feature vector for a track
   * based on its genre, title, and metadata.
   */
  extractTrackVector(track: Track): TasteVector {
    const text = `${track.genre || ''} ${track.title || ''} ${track.artist || ''} ${track.album || ''}`.toLowerCase();

    let energy = 0.5;
    let tempo = 0.5;
    let acoustic = 0.3;
    let hiphop = 0.1;
    let rock = 0.1;
    let electronic = 0.1;
    let pop = 0.1;
    let chill = 0.1;

    // Phonk / Trap / Bass
    if (text.includes('phonk') || text.includes('drift') || text.includes('bass') || text.includes('hardstyle')) {
      energy = 0.95;
      tempo = 0.75;
      electronic = 0.9;
      hiphop = 0.7;
      acoustic = 0.05;
    } 
    // Hip-Hop / Rap
    else if (text.includes('hip-hop') || text.includes('hip hop') || text.includes('rap') || text.includes('рэп') || text.includes('trap')) {
      energy = 0.75;
      tempo = 0.6;
      hiphop = 0.95;
      electronic = 0.4;
      acoustic = 0.2;
    }
    // Rock / Metal / Punk
    else if (text.includes('rock') || text.includes('metal') || text.includes('punk') || text.includes('рок') || text.includes('guitar')) {
      energy = 0.88;
      tempo = 0.7;
      rock = 0.95;
      acoustic = 0.4;
      electronic = 0.2;
    }
    // Electronic / Synthwave / EDM / House
    else if (text.includes('synth') || text.includes('synthwave') || text.includes('edm') || text.includes('house') || text.includes('dance') || text.includes('techno') || text.includes('club')) {
      energy = 0.85;
      tempo = 0.75;
      electronic = 0.95;
      pop = 0.4;
      acoustic = 0.05;
    }
    // Chill / Lo-Fi / Ambient / Relax / Acoustic
    else if (text.includes('lo-fi') || text.includes('lofi') || text.includes('chill') || text.includes('ambient') || text.includes('relax') || text.includes('sleep') || text.includes('piano') || text.includes('acoustic') || text.includes('лаборатория')) {
      energy = 0.25;
      tempo = 0.35;
      chill = 0.95;
      acoustic = 0.85;
      electronic = 0.2;
    }
    // Pop / Indie
    else if (text.includes('pop') || text.includes('поп') || text.includes('indie') || text.includes('инди')) {
      energy = 0.65;
      tempo = 0.55;
      pop = 0.9;
      acoustic = 0.4;
      electronic = 0.3;
    }

    return { energy, tempo, acoustic, hiphop, rock, electronic, pop, chill };
  }

  /**
   * Calculates cosine similarity between two feature vectors: cos(theta) in range [-1, 1], normalized to [0, 1].
   */
  private cosineSimilarity(a: TasteVector, b: TasteVector): number {
    const keys: (keyof TasteVector)[] = ['energy', 'tempo', 'acoustic', 'hiphop', 'rock', 'electronic', 'pop', 'chill'];
    let dot = 0;
    let normA = 0;
    let normB = 0;

    for (const k of keys) {
      dot += a[k] * b[k];
      normA += a[k] * a[k];
      normB += b[k] * b[k];
    }

    if (normA <= 0 || normB <= 0) return 0.5;
    const cos = dot / (Math.sqrt(normA) * Math.sqrt(normB));
    return Math.max(0, Math.min(1, (cos + 1) / 2));
  }

  /**
   * Record when user completes a track (>80% played). Soft positive drift.
   */
  recordTrackCompletion(track: Track) {
    if (track.isLiveStream) return;
    this.recentPlays.set(track.id, Date.now());

    const tVec = this.extractTrackVector(track);
    const cur = this.tasteVector();
    const updated: TasteVector = {
      energy: cur.energy * 0.9 + tVec.energy * 0.1,
      tempo: cur.tempo * 0.9 + tVec.tempo * 0.1,
      acoustic: cur.acoustic * 0.9 + tVec.acoustic * 0.1,
      hiphop: cur.hiphop * 0.9 + tVec.hiphop * 0.1,
      rock: cur.rock * 0.9 + tVec.rock * 0.1,
      electronic: cur.electronic * 0.9 + tVec.electronic * 0.1,
      pop: cur.pop * 0.9 + tVec.pop * 0.1,
      chill: cur.chill * 0.9 + tVec.chill * 0.1,
    };
    this.tasteVector.set(updated);
    this.saveTasteVector(updated);
  }

  /**
   * Record when user likes/favorites a track. Strong positive reinforcement.
   */
  recordTrackLike(track: Track) {
    if (track.isLiveStream) return;
    const tVec = this.extractTrackVector(track);
    const cur = this.tasteVector();
    const updated: TasteVector = {
      energy: cur.energy * 0.8 + tVec.energy * 0.2,
      tempo: cur.tempo * 0.8 + tVec.tempo * 0.2,
      acoustic: cur.acoustic * 0.8 + tVec.acoustic * 0.2,
      hiphop: cur.hiphop * 0.8 + tVec.hiphop * 0.2,
      rock: cur.rock * 0.8 + tVec.rock * 0.2,
      electronic: cur.electronic * 0.8 + tVec.electronic * 0.2,
      pop: cur.pop * 0.8 + tVec.pop * 0.2,
      chill: cur.chill * 0.8 + tVec.chill * 0.2,
    };
    this.tasteVector.set(updated);
    this.saveTasteVector(updated);
  }

  /**
   * Record when user skips a track quickly (<15s). Nudges vector away.
   */
  recordTrackSkip(track: Track) {
    if (track.isLiveStream) return;
    this.recentPlays.set(track.id, Date.now());

    const tVec = this.extractTrackVector(track);
    const cur = this.tasteVector();
    const updated: TasteVector = {
      energy: Math.max(0.05, Math.min(0.95, cur.energy - (tVec.energy - 0.5) * 0.1)),
      tempo: Math.max(0.05, Math.min(0.95, cur.tempo - (tVec.tempo - 0.5) * 0.1)),
      acoustic: Math.max(0.05, Math.min(0.95, cur.acoustic - (tVec.acoustic - 0.5) * 0.1)),
      hiphop: Math.max(0.05, Math.min(0.95, cur.hiphop - (tVec.hiphop - 0.5) * 0.1)),
      rock: Math.max(0.05, Math.min(0.95, cur.rock - (tVec.rock - 0.5) * 0.1)),
      electronic: Math.max(0.05, Math.min(0.95, cur.electronic - (tVec.electronic - 0.5) * 0.1)),
      pop: Math.max(0.05, Math.min(0.95, cur.pop - (tVec.pop - 0.5) * 0.1)),
      chill: Math.max(0.05, Math.min(0.95, cur.chill - (tVec.chill - 0.5) * 0.1)),
    };
    this.tasteVector.set(updated);
    this.saveTasteVector(updated);
  }

  /**
   * Dislike track: adds to blacklist in LibraryService (synced with cloud), nudges vector, and prevents from playing.
   */
  dislikeTrack(trackId: string) {
    this.libraryService.dislikeTrack(trackId);
  }

  isDisliked(trackId: string): boolean {
    return this.libraryService.isDisliked(trackId);
  }

  /**
   * Aggregates all candidate tracks from:
   * 1. Library tracks
   * 2. User playlists
   * 3. Favorites
   */
  getAllLocalCandidates(): Track[] {
    const map = new Map<string, Track>();

    // 1. Library tracks
    for (const t of this.libraryService.tracks()) {
      if (!t.isLiveStream && !this.isDisliked(t.id)) {
        map.set(t.id, t);
      }
    }

    // 2. Playlists
    const plTracksMap = new Map(this.libraryService.tracks().map((t) => [t.id, t]));
    for (const pl of this.libraryService.playlists()) {
      for (const tId of pl.trackIds) {
        const found = plTracksMap.get(tId);
        if (found && !found.isLiveStream && !this.isDisliked(found.id)) {
          map.set(found.id, found);
        }
      }
    }

    return Array.from(map.values());
  }

  /**
   * Adjusts target vector based on selected mood filter
   */
  private getTargetVectorForMood(mood: MixMood): TasteVector {
    const base = { ...this.tasteVector() };
    if (mood === 'energetic') {
      base.energy = Math.max(base.energy, 0.85);
      base.tempo = Math.max(base.tempo, 0.75);
      base.chill = Math.min(base.chill, 0.2);
    } else if (mood === 'chill') {
      base.energy = Math.min(base.energy, 0.35);
      base.chill = Math.max(base.chill, 0.85);
      base.acoustic = Math.max(base.acoustic, 0.65);
    }
    return base;
  }

  /**
   * Scores a track candidate with cosine similarity + favorite boost - fatigue penalty.
   */
  scoreTrack(track: Track, mood: MixMood, targetVec: TasteVector): number {
    if (this.isDisliked(track.id)) return -9999;

    // Mood 'favorites': strictly favorited tracks
    if (mood === 'favorites' && !track.isFavorite) {
      return -9999;
    }

    const trackVec = this.extractTrackVector(track);
    const similarity = this.cosineSimilarity(targetVec, trackVec);

    let score = similarity * 60; // 0..60 points from vector alignment

    // Explicit user affinity
    if (track.isFavorite) {
      score += 25;
    }
    if (track.plays && track.plays > 0) {
      score += Math.min(15, track.plays * 1.5);
    }

    // Language preference
    const lang = this.libraryService.mixConfig().language;
    if (lang === 'ru') {
      const isRu = /[а-яё]/i.test(`${track.title} ${track.artist} ${track.genre || ''}`);
      score += isRu ? 35 : -35;
    } else if (lang === 'en') {
      const isRu = /[а-яё]/i.test(`${track.title} ${track.artist} ${track.genre || ''}`);
      score += !isRu ? 35 : -35;
    }

    // Fatigue penalty (Cooldown)
    const lastPlayed = this.recentPlays.get(track.id);
    if (lastPlayed) {
      const minutesAgo = (Date.now() - lastPlayed) / (1000 * 60);
      if (minutesAgo < 30) {
        score -= 100; // Do not replay within 30 minutes
      } else if (minutesAgo < 120) {
        score -= 40;
      } else if (minutesAgo < 240) {
        score -= 15;
      }
    }

    return score;
  }

  /**
   * Selects N next tracks using Weighted Random Sampling based on scores.
   */
  pickNextTracks(count: number, excludeIds: Set<string> = new Set()): Track[] {
    const candidates = this.getAllLocalCandidates().filter((t) => !excludeIds.has(t.id));
    if (candidates.length === 0) return [];

    const mood = this.currentMood();
    const targetVec = this.getTargetVectorForMood(mood);

    const scored = candidates
      .map((track) => ({
        track,
        score: this.scoreTrack(track, mood, targetVec),
      }))
      .filter((item) => item.score > -50);

    if (scored.length === 0) {
      // Fallback: pick any candidate not recently played
      return candidates.slice(0, count);
    }

    const selected: Track[] = [];
    const used = new Set<string>(excludeIds);

    for (let step = 0; step < count; step++) {
      const pool = scored.filter((s) => !used.has(s.track.id));
      if (pool.length === 0) break;

      // Shift scores so lowest score is positive
      const minScore = Math.min(...pool.map((p) => p.score));
      const baseShift = minScore < 1 ? Math.abs(minScore) + 2 : 0;
      const totalWeight = pool.reduce((sum, p) => sum + (p.score + baseShift), 0);

      let rnd = Math.random() * totalWeight;
      let chosen = pool[0].track;

      for (const item of pool) {
        const w = item.score + baseShift;
        if (rnd <= w) {
          chosen = item.track;
          break;
        }
        rnd -= w;
      }

      selected.push(chosen);
      used.add(chosen.id);
    }

    return selected;
  }

  /**
   * Discovery Engine: Fetches online tracks matching current mood and user taste.
   * Leverages /api/search via LibraryService without blocking UI.
   */
  async fetchOnlineDiscoveryTracks(count = 3, excludeIds: Set<string> = new Set()): Promise<Track[]> {
    if (this.isFetchingDiscovery()) return [];
    if (this.libraryService.mixConfig().source === 'library_only' && this.getAllLocalCandidates().length > 0) return [];
    this.isFetchingDiscovery.set(true);

    try {
      const candidates = this.getAllLocalCandidates();
      const mood = this.currentMood();
      const lang = this.libraryService.mixConfig().language;
      let query = '';

      if (lang === 'ru') {
        if (mood === 'energetic') {
          const ruEnergetic = ['Big Baby Tape', 'OG Buda', 'Kizaru', 'PHARAOH', 'русский дрилл', 'русский фонк'];
          query = ruEnergetic[Math.floor(Math.random() * ruEnergetic.length)];
        } else if (mood === 'chill') {
          const ruChill = ['Miyagi', 'Saluki', 'ANIKV', 'русский лоуфай', 'Zoloto', 'The Limba'];
          query = ruChill[Math.floor(Math.random() * ruChill.length)];
        } else {
          const ruGeneral = ['Miyagi', 'OG Buda', 'Big Baby Tape', 'Saluki', 'Kizaru', 'Markul', 'Scriptonite', 'Instasamka'];
          query = ruGeneral[Math.floor(Math.random() * ruGeneral.length)];
        }
      } else if (lang === 'en') {
        if (mood === 'energetic') {
          const enEnergetic = ['The Weeknd', 'Travis Scott', 'Metro Boomin', 'phonk', 'electronic synthwave', 'rock hits'];
          query = enEnergetic[Math.floor(Math.random() * enEnergetic.length)];
        } else if (mood === 'chill') {
          const enChill = ['lofi hip hop beats', 'Billie Eilish', 'Joji', 'chill rnb', 'acoustic chill'];
          query = enChill[Math.floor(Math.random() * enChill.length)];
        } else {
          const enGeneral = ['The Weeknd', 'Dua Lipa', 'Post Malone', 'Drake', 'Kendrick Lamar', 'Metro Boomin'];
          query = enGeneral[Math.floor(Math.random() * enGeneral.length)];
        }
      } else {
        if (mood === 'energetic') {
          const energeticQueries = ['phonk', 'Big Baby Tape', 'Travis Scott', 'synthwave'];
          query = energeticQueries[Math.floor(Math.random() * energeticQueries.length)];
        } else if (mood === 'chill') {
          const chillQueries = ['lofi chill beats', 'Miyagi', 'acoustic chill', 'Billie Eilish'];
          query = chillQueries[Math.floor(Math.random() * chillQueries.length)];
        } else {
          if (candidates.length > 0) {
            const randomTrack = candidates[Math.floor(Math.random() * candidates.length)];
            query = `${randomTrack.artist}`;
          } else {
            const generalQueries = ['Miyagi', 'The Weeknd', 'OG Buda', 'Post Malone', 'Saluki', 'Dua Lipa'];
            query = generalQueries[Math.floor(Math.random() * generalQueries.length)];
          }
        }
      }

      const results = await this.libraryService.searchOnline(query);
      const filtered = results.filter((t) => 
        !excludeIds.has(t.id) && 
        !this.isDisliked(t.id) &&
        (t.duration === 0 || (t.duration >= 45 && t.duration <= 600))
      );

      return filtered.slice(0, count);
    } catch {
      return [];
    } finally {
      this.isFetchingDiscovery.set(false);
    }
  }

  /**
   * Sets user taste explicitly from quick-start vibes (Cold start)
   */
  setQuickStartVibe(vibe: 'phonk' | 'hiphop' | 'rock' | 'lofi' | 'pop' | 'indie') {
    let vec: TasteVector = { ...DEFAULT_TASTE_VECTOR };
    switch (vibe) {
      case 'phonk':
        vec = { energy: 0.95, tempo: 0.8, acoustic: 0.05, hiphop: 0.8, rock: 0.3, electronic: 0.95, pop: 0.2, chill: 0.1 };
        break;
      case 'hiphop':
        vec = { energy: 0.75, tempo: 0.65, acoustic: 0.2, hiphop: 0.95, rock: 0.2, electronic: 0.5, pop: 0.4, chill: 0.3 };
        break;
      case 'rock':
        vec = { energy: 0.9, tempo: 0.75, acoustic: 0.4, hiphop: 0.1, rock: 0.95, electronic: 0.2, pop: 0.3, chill: 0.1 };
        break;
      case 'lofi':
        vec = { energy: 0.25, tempo: 0.35, acoustic: 0.85, hiphop: 0.4, rock: 0.05, electronic: 0.2, pop: 0.2, chill: 0.95 };
        break;
      case 'pop':
        vec = { energy: 0.7, tempo: 0.6, acoustic: 0.3, hiphop: 0.3, rock: 0.2, electronic: 0.4, pop: 0.95, chill: 0.4 };
        break;
      case 'indie':
        vec = { energy: 0.55, tempo: 0.5, acoustic: 0.65, hiphop: 0.2, rock: 0.6, electronic: 0.3, pop: 0.6, chill: 0.7 };
        break;
    }
    this.tasteVector.set(vec);
    this.saveTasteVector(vec);
  }
}
