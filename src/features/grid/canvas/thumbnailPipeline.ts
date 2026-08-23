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
import { mediaFileUrl } from '../../../shared/lib/mediaUrl';
import type { ThumbnailPipelineEntry } from './thumbnailPipelineTypes';
import {
  computePlanFingerprint,
  sortPlanTilesByViewportDistance,
  type PlanTile,
} from './thumbnailPlan';

export type { PlanTile } from './thumbnailPlan';

export const THUMBNAIL_PIPELINE_REVEAL_MS = 250;
export type { ThumbnailPipelineEntry } from './thumbnailPipelineTypes';

/** If a tile exceeds this size in either axis, load the full original instead of the 512px thumb. */
const FULL_QUALITY_THRESHOLD_PX = 752;

function mediaThumbnailUrl(fileHash: string): string {
  return `media://localhost/thumb/${fileHash}.jpg`;
}

export class ThumbnailPipeline {
  private cache = new Map<string, ThumbnailPipelineEntry>();
  private onDirty: () => void;
  private destroyed = false;
  private totalBytes = 0;
  /** When true, new thumbnails appear instantly without fade animation. */
  suppressAnimation = false;
  /** Timestamp until which animation is suppressed (handles async thumbnail arrival after transition). */
  private suppressUntil = 0;

  // ── Plan deduplication ──
  // Only send the plan when the visible physical-file set actually changes.
  // -1 = never computed; computePlanFingerprint always returns >= 0.
  private lastPlanFingerprint = -1;
  // Reusable array for building plan entries — avoids per-frame allocation.
  private planBuffer: Array<{ fileHash: string; url: string }> = [];

  constructor(onDirty: () => void = () => {}) {
    this.onDirty = onDirty;
    setThumbnailRevealCallback((fileHash, bitmap) => this.handleReveal(fileHash, bitmap));
    setThumbnailErrorCallback((fileHash) => this.handleError(fileHash));
  }

  setOnDirty(onDirty: () => void): void {
    this.onDirty = onDirty;
  }

  /** Suppress fade animation for the next N milliseconds (covers async arrivals after transition). */
  suppressAnimationFor(ms: number): void {
    this.suppressUntil = performance.now() + ms;
  }

  /**
   * Send the current set of visible tiles to the worker.
   * Call once per frame with all physical file hashes in the activation zone.
   * Deduplicates — only posts to the worker when the set actually changes.
   */
  updatePlan(tiles: PlanTile[], viewportCenterY: number): void {
    if (this.destroyed) return;

    // Fingerprint over the incoming layout order — deterministic per tile
    // set, unlike the viewport-distance order below which shifts with every
    // scroll pixel and would defeat deduplication.
    const fingerprint = computePlanFingerprint(tiles, FULL_QUALITY_THRESHOLD_PX);
    if (fingerprint === this.lastPlanFingerprint) return;
    this.lastPlanFingerprint = fingerprint;

    // Plan-entry order is the worker's fetch priority — load tiles nearest
    // the viewport center first. In-place sort of the caller's reusable
    // buffer is safe: drawBase rewrites every slot next frame. Already
    // in-flight fetches are not cancelled on reprioritization — cancelling
    // would thrash on scroll direction reversals.
    sortPlanTilesByViewportDistance(tiles, viewportCenterY);

    // Build entries, reusing buffer
    const buf = this.planBuffer;
    buf.length = tiles.length;
    for (let i = 0; i < tiles.length; i++) {
      const t = tiles[i];
      const isImage = t.mime.startsWith('image/') && t.mime !== 'image/gif';
      const needsFull = isImage && (t.w > FULL_QUALITY_THRESHOLD_PX || t.h > FULL_QUALITY_THRESHOLD_PX);
      const url = needsFull ? mediaFileUrl(t.fileHash, t.mime) : mediaThumbnailUrl(t.fileHash);
      if (buf[i]) {
        buf[i].fileHash = t.fileHash;
        buf[i].url = url;
      } else {
        buf[i] = { fileHash: t.fileHash, url };
      }
    }
    sendThumbnailPlan(buf);
  }

  /** Get a cached entry for drawing. Returns null if no bitmap received yet. */
  get(fileHash: string): ThumbnailPipelineEntry | null {
    return this.cache.get(fileHash) ?? null;
  }

  /** Close bitmaps for tiles no longer in the visible set. */
  evictOutsideVisible(visibleFileHashes: Set<string>): void {
    for (const [fileHash, entry] of this.cache) {
      if (visibleFileHashes.has(fileHash)) continue;
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
    this.lastPlanFingerprint = -1;
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

  private handleReveal(fileHash: string, bitmap: ImageBitmap): void {
    if (this.destroyed) { bitmap.close(); return; }

    let entry = this.cache.get(fileHash);
    if (!entry) {
      entry = { thumb: null, state: 'idle', lastAccessed: 0, bytes: 0, animateIn: false, revealStartedAt: 0 };
      this.cache.set(fileHash, entry);
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

    if (isUpgrade || this.suppressAnimation || performance.now() < this.suppressUntil) {
      entry.animateIn = false;
    } else {
      entry.animateIn = true;
      entry.revealStartedAt = performance.now();
    }

    this.onDirty();
  }

  private handleError(fileHash: string): void {
    let entry = this.cache.get(fileHash);
    if (!entry) {
      entry = { thumb: null, state: 'idle', lastAccessed: 0, bytes: 0, animateIn: false, revealStartedAt: 0 };
      this.cache.set(fileHash, entry);
    }
    entry.state = 'error';
  }
}
