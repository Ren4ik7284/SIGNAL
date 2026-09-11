export interface Track {
  id: string;
  title: string;
  artist: string;
  album?: string;
  duration: number; // in seconds, 0 or -1 if live stream
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
}

export interface Playlist {
  id: string;
  title: string;
  description: string;
  trackIds: string[];
  coverText: string;
}
