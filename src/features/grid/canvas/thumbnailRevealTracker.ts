import { THUMBNAIL_VIEWPORT_DWELL_MS } from './thumbnailTiming';

const REVEAL_DURATION_MS = 500;

interface RevealState {
  startedAt: number | null;
  enteredAt: number;
  instant: boolean;
}

/**
 * Owns reveal identity independently from bitmap residency.
 *
 * An entity keeps its reveal state while it remains in the actual viewport,
 * even if layout or item order changes. Leaving the viewport removes that
 * state, making the next genuine entry eligible for one new reveal.
 */
export class ThumbnailRevealTracker {
  private visible = new Map<string, RevealState>();

  updateViewport(
    entityHashes: ReadonlySet<string>,
    now: number,
    hasBitmap: (entityHash: string) => boolean,
    suppress = false,
  ): void {
    for (const hash of this.visible.keys()) {
      if (!entityHashes.has(hash)) this.visible.delete(hash);
    }

    for (const hash of entityHashes) {
      if (this.visible.has(hash)) continue;
      this.visible.set(hash, {
        startedAt: !suppress && hasBitmap(hash) ? now + THUMBNAIL_VIEWPORT_DWELL_MS : null,
        enteredAt: now,
        instant: suppress,
      });
    }
  }

  onBitmapAvailable(entityHash: string, now: number, suppress = false): void {
    const state = this.visible.get(entityHash);
    if (!state || state.startedAt != null || state.instant) return;
    if (suppress) {
      state.instant = true;
      return;
    }
    state.startedAt = Math.max(now, state.enteredAt + THUMBNAIL_VIEWPORT_DWELL_MS);
  }

  getProgress(entityHash: string, now: number): number {
    const state = this.visible.get(entityHash);
    if (!state || state.instant) return 1;
    if (state.startedAt == null) return 0;
    return Math.min(1, Math.max(0, (now - state.startedAt) / REVEAL_DURATION_MS));
  }

  isAnimating(entityHash: string, now: number): boolean {
    const state = this.visible.get(entityHash);
    return Boolean(
      state
      && !state.instant
      && state.startedAt != null
      && now >= state.startedAt
      && now - state.startedAt < REVEAL_DURATION_MS,
    );
  }

  clear(): void {
    this.visible.clear();
  }
}
