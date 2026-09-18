import {
  Component,
  ElementRef,
  ViewChild,
  OnInit,
  OnDestroy,
  AfterViewInit,
  input,
  signal,
  inject,
  HostListener,
  ViewEncapsulation,
} from '@angular/core';
import { CommonModule } from '@angular/common';
import { FormsModule } from '@angular/forms';
import { AudioService } from '../../services/audio.service';
import { LibraryService } from '../../services/library.service';

export type VisualizerType = 'bars' | 'wave' | 'circle';
export type VisualizerTheme = 'signal' | 'cyan' | 'amber' | 'emerald';

@Component({
  selector: 'app-visualizer',
  standalone: true,
  imports: [CommonModule, FormsModule],
  templateUrl: './visualizer.component.html',
  styleUrl: './visualizer.component.scss',
  encapsulation: ViewEncapsulation.None,
})
export class VisualizerComponent implements OnInit, AfterViewInit, OnDestroy {
  readonly audioService = inject(AudioService);
  readonly libraryService = inject(LibraryService);

  readonly mode = input<'mini' | 'full'>('full');

  @ViewChild('visCanvas') canvasRef!: ElementRef<HTMLCanvasElement>;
  @ViewChild('visualizerContainer') containerRef?: ElementRef<HTMLDivElement>;

  readonly visualType = signal<VisualizerType>('bars');
  readonly colorTheme = signal<VisualizerTheme>('signal');
  readonly sensitivity = signal<number>(1.2);
  readonly isFullscreen = signal<boolean>(false);

  private animationFrameId: number | null = null;
  private resizeObserver: ResizeObserver | null = null;

  // Audio data arrays (fftSize = 256 -> 128 bins)
  private readonly bufferLength = 128;
  private readonly freqData = new Uint8Array(this.bufferLength);
  private readonly timeData = new Uint8Array(this.bufferLength);

  // Peak caps for bars
  private peakCaps: number[] = [];
  private capHoldFrames: number[] = [];
  private zeroDataStreak = 0;
  private syntheticPhase = 0;

  ngOnInit() {
    this.peakCaps = new Array(this.bufferLength).fill(0);
    this.capHoldFrames = new Array(this.bufferLength).fill(0);
  }

  ngAfterViewInit() {
    this.setupResizeObserver();
    this.startRenderLoop();
  }

  ngOnDestroy() {
    this.stopRenderLoop();
    if (this.resizeObserver) {
      this.resizeObserver.disconnect();
    }
  }

  @HostListener('window:keydown', ['$event'])
  onKeyDown(event: KeyboardEvent) {
    if (this.mode() === 'full' && this.audioService.isVisualizerOpen()) {
      if (event.key === 'Escape') {
        event.preventDefault();
        this.close();
      } else if (event.key === 'f' || event.key === 'F' || event.key === 'а' || event.key === 'А') {
        event.preventDefault();
        this.toggleFullscreen();
      } else if (event.key === '1') {
        this.setVisualType('bars');
      } else if (event.key === '2') {
        this.setVisualType('wave');
      } else if (event.key === '3') {
        this.setVisualType('circle');
      }
    }
  }

  setVisualType(type: VisualizerType) {
    this.visualType.set(type);
  }

  setColorTheme(theme: VisualizerTheme) {
    this.colorTheme.set(theme);
  }

  close() {
    if (document.fullscreenElement) {
      document.exitFullscreen().catch(() => {});
    }
    this.audioService.closeVisualizer();
  }

  toggleFullscreen() {
    if (!this.containerRef) return;
    const el = this.containerRef.nativeElement;

    if (!document.fullscreenElement) {
      el.requestFullscreen()
        .then(() => this.isFullscreen.set(true))
        .catch(() => {});
    } else {
      document.exitFullscreen()
        .then(() => this.isFullscreen.set(false))
        .catch(() => {});
    }
  }

  private setupResizeObserver() {
    if (typeof ResizeObserver === 'undefined' || !this.canvasRef) return;
    this.resizeObserver = new ResizeObserver(() => {
      this.adjustCanvasResolution();
    });
    this.resizeObserver.observe(this.canvasRef.nativeElement);
  }

  private adjustCanvasResolution() {
    const canvas = this.canvasRef?.nativeElement;
    if (!canvas) return;

    const rect = canvas.getBoundingClientRect();
    const dpr = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1;

    const width = Math.floor(rect.width * dpr);
    const height = Math.floor(rect.height * dpr);

    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width;
      canvas.height = height;
    }
  }

  private startRenderLoop() {
    const render = () => {
      this.draw();
      this.animationFrameId = requestAnimationFrame(render);
    };
    this.animationFrameId = requestAnimationFrame(render);
  }

  private stopRenderLoop() {
    if (this.animationFrameId !== null) {
      cancelAnimationFrame(this.animationFrameId);
      this.animationFrameId = null;
    }
  }

  private getThemeColors(ctx: CanvasRenderingContext2D, height: number): {
    primary: string;
    secondary: string;
    glow: string;
    gradient: CanvasGradient;
    peak: string;
  } {
    const theme = this.colorTheme();

    let c1 = '#ffffff';
    let c2 = '#71717a';
    let glow = 'rgba(255, 255, 255, 0.4)';
    let peak = '#ffffff';

    if (theme === 'cyan') {
      c1 = '#38bdf8';
      c2 = '#0284c7';
      glow = 'rgba(56, 189, 248, 0.6)';
      peak = '#e0f2fe';
    } else if (theme === 'amber') {
      c1 = '#fbbf24';
      c2 = '#d97706';
      glow = 'rgba(251, 191, 36, 0.6)';
      peak = '#fef3c7';
    } else if (theme === 'emerald') {
      c1 = '#34d399';
      c2 = '#059669';
      glow = 'rgba(52, 211, 153, 0.6)';
      peak = '#d1fae5';
    }

    const grad = ctx.createLinearGradient(0, height, 0, 0);
    grad.addColorStop(0, c2);
    grad.addColorStop(0.7, c1);
    grad.addColorStop(1, peak);

    return { primary: c1, secondary: c2, glow, gradient: grad, peak };
  }

  private draw() {
    const canvas = this.canvasRef?.nativeElement;
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    this.adjustCanvasResolution();

    const w = canvas.width;
    const h = canvas.height;

    ctx.clearRect(0, 0, w, h);

    const isPlaying = this.audioService.isPlaying();
    const hasData = this.audioService.getAudioFrequencyData(this.freqData);
    if (hasData) {
      this.audioService.getAudioTimeDomainData(this.timeData);
    }

    // Check if frequency data is active or zero
    let sum = 0;
    for (let i = 0; i < 32; i++) {
      sum += this.freqData[i];
    }
    const avg = sum / 32;

    if (isPlaying && avg < 3) {
      this.zeroDataStreak++;
    } else {
      this.zeroDataStreak = 0;
    }

    // Organic procedural fallback if playing but audio stream CORS prevented analyser access
    if (isPlaying && this.zeroDataStreak > 10) {
      this.syntheticPhase += 0.05;
      const t = this.syntheticPhase;
      for (let i = 0; i < this.bufferLength; i++) {
        const falloff = Math.max(0.1, 1 - (i / this.bufferLength) * 0.85);
        const bass = Math.sin(t * 2.5 + i * 0.2) * 45 + 50;
        const rhythm = Math.cos(t * 1.2 - i * 0.15) * 35;
        const val = Math.max(0, Math.min(255, (bass + rhythm) * falloff * 1.5));
        // Soft smoothing
        this.freqData[i] = Math.round(this.freqData[i] * 0.7 + val * 0.3);
        this.timeData[i] = Math.round(128 + Math.sin(t * 3 + (i / this.bufferLength) * Math.PI * 4) * (val * 0.4));
      }
    } else if (!isPlaying) {
      // Gentle decay to 0 when paused
      for (let i = 0; i < this.bufferLength; i++) {
        this.freqData[i] = Math.max(0, this.freqData[i] - 6);
        this.timeData[i] = Math.round(128 + (this.timeData[i] - 128) * 0.85);
      }
    }

    const type = this.mode() === 'mini' ? 'bars' : this.visualType();

    if (type === 'bars') {
      this.drawBars(ctx, w, h);
    } else if (type === 'wave') {
      this.drawWaveform(ctx, w, h);
    } else if (type === 'circle') {
      this.drawCircle(ctx, w, h);
    }
  }

  private drawBars(ctx: CanvasRenderingContext2D, w: number, h: number) {
    const isMini = this.mode() === 'mini';
    const numBars = isMini ? 24 : 54;
    const colors = this.getThemeColors(ctx, h);
    const sens = this.sensitivity();

    const barWidth = Math.max(2, (w / numBars) * (isMini ? 0.65 : 0.72));
    const gap = (w - numBars * barWidth) / (numBars + 1);

    const step = Math.max(1, Math.floor(this.bufferLength / numBars));

    for (let i = 0; i < numBars; i++) {
      const dataIdx = Math.min(this.bufferLength - 1, i * step);
      let value = (this.freqData[dataIdx] / 255) * sens;
      value = Math.min(1.0, value);

      const barHeight = Math.max(isMini ? 2 : 4, value * (h * 0.88));
      const x = gap + i * (barWidth + gap);
      const y = h - barHeight;

      // Peak Cap Logic
      if (this.peakCaps[i] === undefined) {
        this.peakCaps[i] = 0;
        this.capHoldFrames[i] = 0;
      }

      if (barHeight >= this.peakCaps[i]) {
        this.peakCaps[i] = barHeight;
        this.capHoldFrames[i] = 12; // hold for 12 frames
      } else {
        if (this.capHoldFrames[i] > 0) {
          this.capHoldFrames[i]--;
        } else {
          this.peakCaps[i] = Math.max(0, this.peakCaps[i] - (isMini ? 1.5 : 2.5));
        }
      }

      // Draw Main Bar
      ctx.fillStyle = colors.gradient;
      if (ctx.roundRect) {
        ctx.beginPath();
        ctx.roundRect(x, y, barWidth, barHeight, isMini ? [2, 2, 0, 0] : [3, 3, 0, 0]);
        ctx.fill();
      } else {
        ctx.fillRect(x, y, barWidth, barHeight);
      }

      // Draw Peak Cap (only in full mode or if bar width is sufficient)
      if (!isMini && this.peakCaps[i] > 4) {
        const peakY = Math.max(0, h - this.peakCaps[i] - 3);
        ctx.fillStyle = colors.peak;
        ctx.fillRect(x, peakY, barWidth, 2);
      }
    }
  }

  private drawWaveform(ctx: CanvasRenderingContext2D, w: number, h: number) {
    const colors = this.getThemeColors(ctx, h);
    const sliceWidth = w / this.bufferLength;
    const sens = this.sensitivity();

    ctx.save();
    ctx.lineWidth = Math.max(2, (w / 400) * 1.5);
    ctx.strokeStyle = colors.primary;
    ctx.shadowColor = colors.glow;
    ctx.shadowBlur = 10;

    // Create gradient fill under curve
    const fillGrad = ctx.createLinearGradient(0, 0, 0, h);
    fillGrad.addColorStop(0, 'rgba(255, 255, 255, 0.15)');
    fillGrad.addColorStop(1, 'rgba(0, 0, 0, 0.0)');

    ctx.beginPath();
    ctx.moveTo(0, h / 2);

    let x = 0;
    for (let i = 0; i < this.bufferLength; i++) {
      const v = (this.timeData[i] - 128) / 128; // -1.0 to 1.0
      const scaledV = v * sens;
      const y = h / 2 + scaledV * (h * 0.42);

      if (i === 0) {
        ctx.moveTo(x, y);
      } else {
        // Smooth curve
        const prevX = x - sliceWidth;
        const prevV = ((this.timeData[i - 1] - 128) / 128) * sens;
        const prevY = h / 2 + prevV * (h * 0.42);
        const midX = (prevX + x) / 2;
        const midY = (prevY + y) / 2;
        ctx.quadraticCurveTo(prevX, prevY, midX, midY);
      }
      x += sliceWidth;
    }
    ctx.lineTo(w, h / 2);
    ctx.stroke();

    // Fill under wave
    ctx.lineTo(w, h);
    ctx.lineTo(0, h);
    ctx.closePath();
    ctx.fillStyle = fillGrad;
    ctx.fill();

    ctx.restore();
  }

  private drawCircle(ctx: CanvasRenderingContext2D, w: number, h: number) {
    const colors = this.getThemeColors(ctx, h);
    const centerX = w / 2;
    const centerY = h / 2;
    const baseRadius = Math.min(w, h) * 0.22;
    const numBars = 64;
    const sens = this.sensitivity();

    // Bass energy for center pulse
    let bassSum = 0;
    for (let i = 0; i < 8; i++) {
      bassSum += this.freqData[i];
    }
    const pulse = (bassSum / 8 / 255) * 14 * sens;
    const currentRadius = baseRadius + pulse;

    ctx.save();
    ctx.translate(centerX, centerY);

    // Inner glowing ring
    ctx.beginPath();
    ctx.arc(0, 0, Math.max(10, currentRadius - 6), 0, Math.PI * 2);
    ctx.strokeStyle = colors.secondary;
    ctx.lineWidth = 1.5;
    ctx.stroke();

    ctx.beginPath();
    ctx.arc(0, 0, currentRadius, 0, Math.PI * 2);
    ctx.strokeStyle = colors.primary;
    ctx.shadowColor = colors.glow;
    ctx.shadowBlur = 12;
    ctx.lineWidth = 2;
    ctx.stroke();

    // Draw radial frequency spikes
    const angleStep = (Math.PI * 2) / numBars;
    const step = Math.max(1, Math.floor(this.bufferLength / numBars));

    for (let i = 0; i < numBars; i++) {
      const dataIdx = Math.min(this.bufferLength - 1, i * step);
      const val = Math.min(1.0, (this.freqData[dataIdx] / 255) * sens);
      const spikeLen = Math.max(4, val * (Math.min(w, h) * 0.24));

      const angle = i * angleStep - Math.PI / 2;
      const cos = Math.cos(angle);
      const sin = Math.sin(angle);

      const x1 = cos * (currentRadius + 4);
      const y1 = sin * (currentRadius + 4);
      const x2 = cos * (currentRadius + 4 + spikeLen);
      const y2 = sin * (currentRadius + 4 + spikeLen);

      ctx.beginPath();
      ctx.moveTo(x1, y1);
      ctx.lineTo(x2, y2);
      ctx.strokeStyle = colors.primary;
      ctx.lineWidth = Math.max(2, (w / 500) * 2.2);
      ctx.lineCap = 'round';
      ctx.stroke();

      // Outer peak dot
      if (val > 0.4) {
        ctx.beginPath();
        ctx.arc(cos * (currentRadius + spikeLen + 8), sin * (currentRadius + spikeLen + 8), 1.5, 0, Math.PI * 2);
        ctx.fillStyle = colors.peak;
        ctx.fill();
      }
    }

    ctx.restore();
  }

  formatTime(seconds: number): string {
    if (isNaN(seconds) || seconds < 0 || !isFinite(seconds)) return '0:00';
    const mins = Math.floor(seconds / 60);
    const secs = Math.floor(seconds % 60);
    return `${mins}:${secs < 10 ? '0' : ''}${secs}`;
  }
}
