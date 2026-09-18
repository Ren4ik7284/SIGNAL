import { Component, EventEmitter, Output, inject, ViewEncapsulation } from '@angular/core';
import { CommonModule } from '@angular/common';
import { FormsModule } from '@angular/forms';
import { LibraryService } from '../../services/library.service';

@Component({
  selector: 'app-header-bar',
  standalone: true,
  imports: [CommonModule, FormsModule],
  templateUrl: './header.component.html',
  styleUrl: './header.component.scss',
  encapsulation: ViewEncapsulation.None,
})
export class HeaderComponent {
  readonly libraryService = inject(LibraryService);

  @Output() openAddModal = new EventEmitter<void>();
  @Output() openOnlineSearch = new EventEmitter<string>();
}
