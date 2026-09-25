export interface Track {
  id: string;
  title: string;
  artist: string;
  album?: string;
  duration: number;
  audioUrl: string;
  coverUrl?: string;
  genre: string;
  year?: number;
  format: 'mp3' | 'wav' | 'flac' | 'ogg' | 'm4a' | 'stream';
  bitrate?: string;
  plays: number;
  isFavorite: boolean;
  addedAt: string;
  isLocalUpload?: boolean;
  isLiveStream?: boolean;
  isOffline?: boolean;
  playlistOnly?: boolean;
}

export interface Playlist {
  id: string;
  title: string;
  description: string;
  trackIds: string[];
  coverText: string;
}

export interface RadioStation {
  id: string;
  name: string;
  streamUrl: string;
  genre: string;
  country?: string;
  bitrate?: string;
  favicon?: string;
  isCustom?: boolean;
}

export type MixMood = 'all' | 'energetic' | 'chill' | 'favorites';
export type MixSource = 'balanced' | 'library_only' | 'discovery_heavy';
export type MixLanguage = 'all' | 'ru' | 'en';

export interface MixConfig {
  mood: MixMood;
  source: MixSource;
  language: MixLanguage;
}

export const DEFAULT_MIX_CONFIG: MixConfig = {
  mood: 'all',
  source: 'balanced',
  language: 'all',
};
