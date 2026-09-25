import { Component, EventEmitter, Output, inject, signal, ViewEncapsulation } from '@angular/core';
import { CommonModule } from '@angular/common';
import { FormsModule } from '@angular/forms';
import { AudioService } from '../../services/audio.service';
import { LibraryService } from '../../services/library.service';
import { OfflineService } from '../../services/offline.service';
import { VisualizerComponent } from '../visualizer/visualizer.component';
import { Track } from '../../models/track.model';

@Component({
  selector: 'app-player-bar',
  standalone: true,
  imports: [CommonModule, FormsModule, VisualizerComponent],
  templateUrl: './player-bar.component.html',
  styleUrl: './player-bar.component.scss',
  encapsulation: ViewEncapsulation.None,
})
export class PlayerBarComponent {
  readonly audioService = inject(AudioService);
  readonly libraryService = inject(LibraryService);
  readonly offlineService = inject(OfflineService);

  @Output() toggleQueueDrawer = new EventEmitter<void>();
  @Output() expandMobilePlayer = new EventEmitter<void>();
  @Output() openAddToPlaylist = new EventEmitter<Track>();

  onLeftClick() {
    if (typeof window !== 'undefined' && window.innerWidth <= 768) {
      this.expandMobilePlayer.emit();
    }
  }

  toggleFavorite(track: Track) {
    const isNowFav = this.libraryService.toggleFavorite(track.id, track);
    this.audioService.updateTrackFavoriteStatus(track.id, isNowFav, track);
    if (isNowFav) {
      this.audioService.recService.recordTrackLike(track);
    }
  }

  async toggleOfflineTrack(track: Track) {
    if (this.offlineService.isTrackOffline(track.id)) {
      await this.offlineService.removeTrackOffline(track.id);
      this.libraryService.updateTrackOfflineStatus(track.id, false);
    } else {
      const ok = await this.offlineService.saveTrackOffline(track);
      if (ok) {
        this.libraryService.addTrackToLibrary(track);
        this.libraryService.updateTrackOfflineStatus(track.id, true);
      }
    }
  }

  // Scrubber Dragging State
  readonly isScrubbing = signal<boolean>(false);
  readonly scrubTime = signal<number>(0);

  onScrubberInput(val: number) {
    this.isScrubbing.set(true);
    this.scrubTime.set(val);
  }

  onScrubberChange(val: number) {
    this.isScrubbing.set(false);
    this.audioService.seek(val);
  }

  formatTime(seconds: number): string {
    if (isNaN(seconds) || seconds < 0 || !isFinite(seconds)) return '0:00';
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins}:${secs < 10 ? '0' : ''}${secs}`;
  }
}
