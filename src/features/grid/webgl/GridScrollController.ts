export type ScrollPlatformProfile = 'mac' | 'windows' | 'generic';

export interface ScrollbarVisualState {
  trackX: number;
  trackY: number;
  trackWidth: number;
  trackHeight: number;
  thumbY: number;
  thumbHeight: number;
  opacity: number;
  showTrack: boolean;
}

interface GridScrollControllerOptions {
  platform: ScrollPlatformProfile;
  onChange: (scrollOffset: number) => void;
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

function normalizeWheelDelta(deltaY: number, deltaMode: number): number {
  if (deltaMode === 1) return deltaY * 40;
  if (deltaMode === 2) return deltaY * (window.innerHeight || 800);
  return deltaY;
}

export class GridScrollController {
  private readonly platform: ScrollPlatformProfile;
  private readonly onChange: (scrollOffset: number) => void;
  private viewportHeight = 0;
  private totalHeight = 0;
  private scrollOffset = 0;
  private interactive = true;
  private draggingThumb = false;
  private dragThumbOffset = 0;

  constructor(options: GridScrollControllerOptions) {
    this.platform = options.platform;
    this.onChange = options.onChange;
  }

  setInteractive(interactive: boolean): void {
    this.interactive = interactive;
  }

  setMetrics(totalHeight: number, viewportHeight: number): void {
    this.totalHeight = Math.max(0, totalHeight);
    this.viewportHeight = Math.max(0, viewportHeight);
    this.setScrollOffset(this.scrollOffset);
  }

  setScrollOffset(nextOffset: number): void {
    const maxScroll = this.getMaxScrollOffset();
    const clamped = clamp(nextOffset, 0, maxScroll);
    if (Math.abs(clamped - this.scrollOffset) < 0.25) return;
    this.scrollOffset = clamped;
    this.onChange(clamped);
  }

  getScrollOffset(): number {
    return this.scrollOffset;
  }

  getMaxScrollOffset(): number {
    return Math.max(0, this.totalHeight - this.viewportHeight);
  }

  handleWheel(deltaY: number, deltaMode: number): void {
    if (!this.interactive) return;
    const normalized = normalizeWheelDelta(deltaY, deltaMode);
    const adjusted = this.platform === 'windows' && deltaMode === 1
      ? normalized
      : normalized;
    if (adjusted === 0) return;
    this.setScrollOffset(this.scrollOffset + adjusted);
  }

  getScrollbarState(viewportWidth: number): ScrollbarVisualState {
    const trackWidth = 8;
    const inset = 0;
    const trackHeight = Math.max(0, this.viewportHeight - inset * 2);
    const trackX = Math.max(0, viewportWidth - trackWidth - inset);
    const trackY = inset;
    const maxScroll = this.getMaxScrollOffset();
    const ratio = this.totalHeight > 0 ? this.viewportHeight / this.totalHeight : 1;
    const thumbHeight = maxScroll <= 0
      ? trackHeight
      : clamp(trackHeight * ratio, 28, trackHeight);
    const thumbTravel = Math.max(0, trackHeight - thumbHeight);
    const thumbY = maxScroll <= 0
      ? trackY
      : trackY + (this.scrollOffset / maxScroll) * thumbTravel;
    const opacity = maxScroll <= 0 ? 0 : 1;

    return {
      trackX,
      trackY,
      trackWidth,
      trackHeight,
      thumbY,
      thumbHeight,
      opacity,
      showTrack: maxScroll > 0,
    };
  }

  beginPointerInteraction(localX: number, localY: number, viewportWidth: number): boolean {
    if (!this.interactive) return false;
    const state = this.getScrollbarState(viewportWidth);
    const withinTrack = localX >= state.trackX
      && localX <= state.trackX + state.trackWidth
      && localY >= state.trackY
      && localY <= state.trackY + state.trackHeight;
    if (!withinTrack || state.opacity <= 0) return false;

    const withinThumb = localY >= state.thumbY && localY <= state.thumbY + state.thumbHeight;
    if (withinThumb) {
      this.draggingThumb = true;
      this.dragThumbOffset = localY - state.thumbY;
      return true;
    }

    const pageDelta = this.viewportHeight * 0.85;
    if (localY < state.thumbY) {
      this.setScrollOffset(this.scrollOffset - pageDelta);
    } else {
      this.setScrollOffset(this.scrollOffset + pageDelta);
    }
    return true;
  }

  handlePointerMove(localY: number, viewportWidth: number): boolean {
    if (!this.draggingThumb) return false;
    const state = this.getScrollbarState(viewportWidth);
    const thumbTravel = Math.max(0, state.trackHeight - state.thumbHeight);
    const nextThumbY = clamp(localY - this.dragThumbOffset, state.trackY, state.trackY + thumbTravel);
    const thumbProgress = thumbTravel <= 0 ? 0 : (nextThumbY - state.trackY) / thumbTravel;
    this.setScrollOffset(thumbProgress * this.getMaxScrollOffset());
    return true;
  }

  endPointerInteraction(): void {
    this.draggingThumb = false;
  }
}

export function detectScrollPlatformProfile(): ScrollPlatformProfile {
  const userAgentDataPlatform = (navigator as Navigator & { userAgentData?: { platform?: string } }).userAgentData?.platform;
  const platform = (userAgentDataPlatform ?? navigator.platform ?? '').toLowerCase();
  if (platform.includes('mac')) return 'mac';
  if (platform.includes('win')) return 'windows';
  return 'generic';
}
