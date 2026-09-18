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
