import { Component, EventEmitter, Output, inject, signal, ViewEncapsulation } from '@angular/core';
import { CommonModule } from '@angular/common';
import { FormsModule } from '@angular/forms';
import { LibraryService } from '../../services/library.service';
import { AuthService } from '../../services/auth.service';
import { AudioService } from '../../services/audio.service';

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
  readonly authService = inject(AuthService);
  readonly audioService = inject(AudioService);

  @Output() openAddModal = new EventEmitter<void>();
  @Output() openOnlineSearch = new EventEmitter<string>();
  @Output() openAuthModal = new EventEmitter<void>();
  @Output() openWrappedModal = new EventEmitter<void>();
  @Output() openHistoryModal = new EventEmitter<void>();

  readonly isUserMenuOpen = signal<boolean>(false);

  toggleUserMenu() {
    this.isUserMenuOpen.update((v) => !v);
  }

  closeUserMenu() {
    this.isUserMenuOpen.set(false);
  }

  handleWrappedClick() {
    this.closeUserMenu();
    this.openWrappedModal.emit();
  }

  handleHistoryClick() {
    this.closeUserMenu();
    this.openHistoryModal.emit();
  }

  handleLogout() {
    this.closeUserMenu();
    this.audioService.stopPlayback();
    this.authService.logout();
    this.libraryService.onUserLoggedOut();
  }
}
