import { Component, EventEmitter, Output, inject, signal } from '@angular/core';
import { CommonModule } from '@angular/common';
import { FormsModule } from '@angular/forms';
import { AudioService } from '../../services/audio.service';
import { LibraryService } from '../../services/library.service';
import { OfflineService } from '../../services/offline.service';
import { Track } from '../../models/track.model';

@Component({
  selector: 'app-player-bar',
  standalone: true,
  imports: [CommonModule, FormsModule],
  templateUrl: './player-bar.component.html',
  styleUrl: './player-bar.component.scss',
})
export class PlayerBarComponent {
  readonly audioService = inject(AudioService);
  readonly libraryService = inject(LibraryService);
  readonly offlineService = inject(OfflineService);

  @Output() toggleQueueDrawer = new EventEmitter<void>();

  async toggleOfflineTrack(track: Track) {
    if (this.offlineService.isTrackOffline(track.id)) {
      await this.offlineService.removeTrackOffline(track.id);
    } else {
      await this.offlineService.saveTrackOffline(track);
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
