/**
 * Thumbnail pipeline — thin main-thread layer over the decode worker.
 *
 * The worker owns loading, caching, concurrency, and reveal staggering.
 * This class just:
 *  1. Sends the plan (visible hashes) to the worker each frame.
 *  2. Receives revealed bitmaps and stores them for drawing.
 *  3. Handles eviction of transferred bitmaps.
 *
 * The main thread never waits on the decoder. Bitmap arrival IS the
 * signal to start fading in.
 */

import {
  sendThumbnailPlan,
  clearThumbnailWorker,
  setThumbnailRevealCallback,
  setThumbnailErrorCallback,
  terminateThumbnailWorker,
} from './thumbnailDecodeClient';
import type { ThumbnailPipelineEntry } from './thumbnailPipelineTypes';

export const THUMBNAIL_PIPELINE_REVEAL_MS = 250;
export type { ThumbnailPipelineEntry } from './thumbnailPipelineTypes';

function mediaThumbnailUrl(hash: string): string {
  return `media://localhost/thumb/${hash}.jpg`;
}

export class ThumbnailPipeline {
  private cache = new Map<string, ThumbnailPipelineEntry>();
  private onDirty: () => void;
  private destroyed = false;
  private totalBytes = 0;

  constructor(onDirty: () => void = () => {}) {
    this.onDirty = onDirty;
    setThumbnailRevealCallback((hash, bitmap) => this.handleReveal(hash, bitmap));
    setThumbnailErrorCallback((hash) => this.handleError(hash));
  }

  setOnDirty(onDirty: () => void): void {
    this.onDirty = onDirty;
  }

  /**
   * Send the current set of visible tiles to the worker.
   * Call once per frame with all hashes in the activation zone.
   * The worker diffs against its previous plan and starts/cancels loads.
   */
  updatePlan(hashes: string[]): void {
    if (this.destroyed) return;
    sendThumbnailPlan(hashes.map(h => ({ hash: h, url: mediaThumbnailUrl(h) })));
  }

  /** Get a cached entry for drawing. Returns null if no bitmap received yet. */
  get(hash: string): ThumbnailPipelineEntry | null {
    return this.cache.get(hash) ?? null;
  }

  /** Close bitmaps for tiles no longer in the visible set. */
  evictOutsideVisible(visibleHashes: Set<string>): void {
    for (const [hash, entry] of this.cache) {
      if (visibleHashes.has(hash)) continue;
      if (entry.thumb) {
        this.totalBytes -= entry.bytes;
        entry.thumb.close();
        entry.thumb = null;
        entry.bytes = 0;
      }
      entry.animateIn = false;
      entry.revealStartedAt = 0;
      entry.state = 'idle';
    }
  }

  /** Tear down — scope change or unmount. */
  clear(): void {
    this.destroyed = true;
    clearThumbnailWorker();
    for (const entry of this.cache.values()) entry.thumb?.close();
    this.cache.clear();
    this.totalBytes = 0;
  }

  /** Destroy worker entirely (unmount). */
  destroy(): void {
    this.clear();
    terminateThumbnailWorker();
  }

  // ── Worker callbacks ────────────────────────────────────────────

  private handleReveal(hash: string, bitmap: ImageBitmap): void {
    if (this.destroyed) { bitmap.close(); return; }

    let entry = this.cache.get(hash);
    if (!entry) {
      entry = { thumb: null, state: 'idle', lastAccessed: 0, bytes: 0, animateIn: false, revealStartedAt: 0 };
      this.cache.set(hash, entry);
    }

    const isUpgrade = entry.thumb != null;
    if (entry.thumb) {
      this.totalBytes -= entry.bytes;
      entry.thumb.close();
    }

    entry.thumb = bitmap;
    entry.bytes = bitmap.width * bitmap.height * 4;
    this.totalBytes += entry.bytes;
    entry.state = 'shown';

    if (isUpgrade) {
      entry.animateIn = false;
    } else {
      entry.animateIn = true;
      entry.revealStartedAt = performance.now();
    }

    this.onDirty();
  }

  private handleError(hash: string): void {
    let entry = this.cache.get(hash);
    if (!entry) {
      entry = { thumb: null, state: 'idle', lastAccessed: 0, bytes: 0, animateIn: false, revealStartedAt: 0 };
      this.cache.set(hash, entry);
    }
    entry.state = 'error';
  }
}
