import { THUMBNAIL_REVEAL_DELAY_MS, THUMBNAIL_REVEAL_DURATION_MS } from './thumbnailTiming';

interface RevealState {
  startedAt: number | null;
  enteredAt: number;
}

/**
 * Owns reveal identity independently from bitmap residency.
 *
 * An entity keeps its reveal state while it remains within 500px of the viewport,
 * even if layout or item order changes. Leaving that expanded region removes its
 * state. Every genuine entry waits 100 ms and reveals over 500 ms, including
 * cached/prefetched bitmaps. Loading and revealing are separate decisions.
 */
export class ThumbnailRevealTracker {
  private visible = new Map<string, RevealState>();
  private wakeTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(private readonly onWake: () => void = () => {}) {}

  updateViewport(
    entityHashes: ReadonlySet<string>,
    hasBitmap: (entityHash: string) => boolean,
    now = performance.now(),
  ): void {
    for (const hash of this.visible.keys()) {
      if (!entityHashes.has(hash)) this.visible.delete(hash);
    }

    for (const hash of entityHashes) {
      if (this.visible.has(hash)) continue;
      this.visible.set(hash, {
        enteredAt: now,
        startedAt: hasBitmap(hash) ? now + THUMBNAIL_REVEAL_DELAY_MS : null,
      });
    }
    this.scheduleWake(now);
  }

  onBitmapAvailable(entityHash: string, now: number): void {
    const state = this.visible.get(entityHash);
    if (!state || state.startedAt != null) return;
    state.startedAt = Math.max(now, state.enteredAt + THUMBNAIL_REVEAL_DELAY_MS);
    this.scheduleWake(now);
  }

  getProgress(entityHash: string, now: number): number {
    const state = this.visible.get(entityHash);
    if (!state) return 0;
    if (state.startedAt == null) return 0;
    return Math.min(1, Math.max(0, (now - state.startedAt) / THUMBNAIL_REVEAL_DURATION_MS));
  }

  isAnimating(entityHash: string, now: number): boolean {
    const state = this.visible.get(entityHash);
    return Boolean(
      state
      && state.startedAt != null
      && now >= state.startedAt
      && now - state.startedAt < THUMBNAIL_REVEAL_DURATION_MS,
    );
  }

  clear(): void {
    if (this.wakeTimer != null) clearTimeout(this.wakeTimer);
    this.wakeTimer = null;
    this.visible.clear();
  }

  private scheduleWake(now: number): void {
    if (this.wakeTimer != null) clearTimeout(this.wakeTimer);
    this.wakeTimer = null;
    let next = Infinity;
    for (const state of this.visible.values()) {
      if (state.startedAt != null && state.startedAt > now) next = Math.min(next, state.startedAt);
    }
    if (!Number.isFinite(next)) return;
    // One timer per visible grid, not a RAF loop or a timer for every image.
    this.wakeTimer = setTimeout(() => {
      this.wakeTimer = null;
      this.onWake();
    }, Math.max(1, Math.ceil(next - now)));
  }
}
