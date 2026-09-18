import { Component, EventEmitter, Input, Output, inject, ViewEncapsulation } from '@angular/core';
import { CommonModule } from '@angular/common';
import { LibraryService } from '../../services/library.service';

@Component({
  selector: 'app-sidebar',
  standalone: true,
  imports: [CommonModule],
  templateUrl: './sidebar.component.html',
  styleUrl: './sidebar.component.scss',
  encapsulation: ViewEncapsulation.None,
})
export class SidebarComponent {
  readonly libraryService = inject(LibraryService);

  @Input() activeTab: 'all' | 'favorites' | 'uploads' | 'streams' | 'playlist' | 'offline' = 'all';

  @Output() viewChange = new EventEmitter<{ view: string; playlistId?: string }>();
  @Output() createPlaylist = new EventEmitter<void>();
}
