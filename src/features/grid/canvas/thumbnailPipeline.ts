/**
 * Thumbnail pipeline — thin main-thread layer over the decode worker.
 *
 * The worker owns loading, cancellation, and decoding.
 * This class just:
 *  1. Sends the plan (visible hashes) to the worker each frame.
 *  2. Receives decoded bitmaps and stores them for drawing.
 *  3. Handles eviction of transferred bitmaps.
 *
 * The main thread never waits on the decoder. Reveal identity is owned
 * separately by ThumbnailRevealTracker.
 */

import {
  sendThumbnailPlan,
  clearThumbnailWorker,
  setThumbnailBitmapCallback,
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

export type { ThumbnailPipelineEntry } from './thumbnailPipelineTypes';

/** If a tile exceeds this size in either axis, load the full original instead of the 512px thumb. */
const FULL_QUALITY_THRESHOLD_PX = 752;

function mediaThumbnailUrl(hash: string): string {
  return `media://localhost/thumb/${hash}.jpg`;
}

export class ThumbnailPipeline {
  private cache = new Map<string, ThumbnailPipelineEntry>();
  private onDirty: () => void;
  private onBitmapAvailable: (hash: string) => void;
  private destroyed = false;
  private totalBytes = 0;

  // ── Plan deduplication ──
  // Only send plan to worker when the visible hash set actually changes.
  // -1 = never computed; computePlanFingerprint always returns >= 0.
  private lastPlanFingerprint = -1;
  // Reusable array for building plan entries — avoids per-frame allocation.
  private planBuffer: Array<{ hash: string; url: string }> = [];

  constructor(onDirty: () => void = () => {}, onBitmapAvailable: (hash: string) => void = () => {}) {
    this.onDirty = onDirty;
    this.onBitmapAvailable = onBitmapAvailable;
    setThumbnailBitmapCallback((hash, bitmap) => this.handleBitmap(hash, bitmap));
    setThumbnailErrorCallback((hash) => this.handleError(hash));
  }

  setOnDirty(onDirty: () => void): void {
    this.onDirty = onDirty;
  }

  /**
   * Send the current set of visible tiles to the worker.
   * Call once per frame with all hashes in the activation zone.
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
      const url = needsFull ? mediaFileUrl(t.hash, t.mime) : mediaThumbnailUrl(t.hash);
      if (buf[i]) {
        buf[i].hash = t.hash;
        buf[i].url = url;
      } else {
        buf[i] = { hash: t.hash, url };
      }
    }
    sendThumbnailPlan(buf);
  }

  /** Get a cached entry for drawing. Returns null if no bitmap received yet. */
  get(hash: string): ThumbnailPipelineEntry | null {
    return this.cache.get(hash) ?? null;
  }

  /** Close bitmaps for tiles no longer in the decode activation zone. */
  evictOutsideActive(activeHashes: Set<string>): void {
    for (const [hash, entry] of this.cache) {
      if (activeHashes.has(hash)) continue;
      if (entry.thumb) {
        this.totalBytes -= entry.bytes;
        entry.thumb.close();
        entry.thumb = null;
        entry.bytes = 0;
      }
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

  private handleBitmap(hash: string, bitmap: ImageBitmap): void {
    if (this.destroyed) { bitmap.close(); return; }

    let entry = this.cache.get(hash);
    if (!entry) {
      entry = { thumb: null, state: 'idle', lastAccessed: 0, bytes: 0 };
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

    if (!isUpgrade) this.onBitmapAvailable(hash);
    this.onDirty();
  }

  private handleError(hash: string): void {
    let entry = this.cache.get(hash);
    if (!entry) {
      entry = { thumb: null, state: 'idle', lastAccessed: 0, bytes: 0 };
      this.cache.set(hash, entry);
    }
    entry.state = 'error';
  }
}
