import { Component, EventEmitter, Input, Output, inject } from '@angular/core';
import { CommonModule } from '@angular/common';
import { LibraryService } from '../../services/library.service';

@Component({
  selector: 'app-sidebar',
  standalone: true,
  imports: [CommonModule],
  templateUrl: './sidebar.component.html',
  styleUrl: './sidebar.component.scss',
})
export class SidebarComponent {
  readonly libraryService = inject(LibraryService);

  @Input() activeTab: 'all' | 'favorites' | 'uploads' | 'streams' | 'playlist' = 'all';

  @Output() viewChange = new EventEmitter<{ view: string; playlistId?: string }>();
  @Output() createPlaylist = new EventEmitter<void>();
}
