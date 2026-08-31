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
  ThumbnailDecodeClient,
  type ThumbnailDecodeFailure,
  type ThumbnailDecodePlanEntry,
  type ThumbnailDecodeQuality,
} from './thumbnailDecodeClient';
import { mediaFileUrl } from '../../../shared/lib/mediaUrl';
import type { ThumbnailPipelineEntry } from './thumbnailPipelineTypes';
import {
  computePlanFingerprint,
  shouldLoadFullQualityOriginal,
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
  private readonly decoder: ThumbnailDecodeClient;
  private readonly revisions = new Map<string, number>();
  private readonly installedQuality = new Map<string, ThumbnailDecodeQuality>();
  private readonly fullEligibleHashes = new Set<string>();
  private activeHashes = new Set<string>();
  private readonly pendingFullBitmaps = new Map<string, ImageBitmap>();
  private fullAdmissionFrame: number | null = null;

  // ── Plan deduplication ──
  // Only send plan to worker when the visible hash set actually changes.
  // -1 = never computed; computePlanFingerprint always returns >= 0.
  private lastPlanFingerprint = -1;
  // Reusable array for building plan entries — avoids per-frame allocation.
  private planBuffer: ThumbnailDecodePlanEntry[] = [];

  constructor(
    onDirty: () => void = () => {},
    onBitmapAvailable: (hash: string) => void = () => {},
    private readonly scheduleFrame: (callback: FrameRequestCallback) => number =
      (callback) => window.requestAnimationFrame(callback),
    private readonly cancelFrame: (handle: number) => void =
      (handle) => window.cancelAnimationFrame(handle),
  ) {
    this.onDirty = onDirty;
    this.onBitmapAvailable = onBitmapAvailable;
    this.decoder = new ThumbnailDecodeClient(
      (hash, bitmap, quality) => this.handleBitmap(hash, bitmap, quality),
      (hash, quality, failure) => this.handleError(hash, quality, failure),
    );
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
    this.activeHashes = new Set(tiles.map((tile) => tile.fileHash));

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
      const fullEligible = shouldLoadFullQualityOriginal(t, FULL_QUALITY_THRESHOLD_PX);
      if (fullEligible) this.fullEligibleHashes.add(t.fileHash);
      else this.fullEligibleHashes.delete(t.fileHash);
      // Large tiles are progressive: show the inexpensive thumbnail first,
      // then replace it with the original. A failed original decode must never
      // leave a tile blank when its thumbnail is already available.
      const needsFull = fullEligible && this.installedQuality.has(t.fileHash);
      const revision = this.revisions.get(t.fileHash) ?? 0;
      const url = needsFull
        ? mediaFileUrl(t.fileHash, t.mime)
        : `${mediaThumbnailUrl(t.fileHash)}?v=${revision}`;
      const quality: ThumbnailDecodeQuality = needsFull ? 'full' : 'thumbnail';
      if (!needsFull) this.discardPendingFullBitmap(t.fileHash);
      if (buf[i]) {
        buf[i].fileHash = t.fileHash;
        buf[i].url = url;
        buf[i].quality = quality;
      } else {
        buf[i] = { fileHash: t.fileHash, url, quality };
      }
    }
    this.decoder.sendPlan(buf);
  }

  /** Get a cached entry for drawing. Returns null if no bitmap received yet. */
  get(hash: string): ThumbnailPipelineEntry | null {
    return this.cache.get(hash) ?? null;
  }

  invalidate(hash: string): void {
    // Cloud restore can finish hundreds of off-screen derivatives per second.
    // A future plan will request their final URL directly; only active tiles
    // need cache busting and a repaint now.
    if (!this.activeHashes.has(hash)) return;
    // Keep a usable bitmap on screen until its replacement has decoded.
    // Removing it here exposes a placeholder frame for every background
    // thumbnail refresh and makes active subscription grids visibly flash.
    this.revisions.set(hash, (this.revisions.get(hash) ?? 0) + 1);
    this.lastPlanFingerprint = -1;
    this.discardPendingFullBitmap(hash);
    this.decoder.invalidate(hash);
    this.onDirty();
  }

  /** Close bitmaps for tiles no longer in the decode activation zone. */
  evictOutsideActive(activeHashes: Set<string>): void {
    for (const hash of this.pendingFullBitmaps.keys()) {
      if (!activeHashes.has(hash)) this.discardPendingFullBitmap(hash);
    }
    for (const [hash, entry] of this.cache) {
      if (activeHashes.has(hash)) continue;
      if (entry.thumb) {
        this.totalBytes -= entry.bytes;
        entry.thumb.close();
        entry.thumb = null;
        entry.bytes = 0;
      }
      entry.state = 'idle';
      this.installedQuality.delete(hash);
      this.fullEligibleHashes.delete(hash);
    }
  }

  /** Tear down — scope change or unmount. */
  clear(): void {
    this.destroyed = true;
    this.lastPlanFingerprint = -1;
    this.activeHashes.clear();
    this.installedQuality.clear();
    this.fullEligibleHashes.clear();
    this.decoder.clear();
    if (this.fullAdmissionFrame != null) {
      this.cancelFrame(this.fullAdmissionFrame);
      this.fullAdmissionFrame = null;
    }
    for (const bitmap of this.pendingFullBitmaps.values()) bitmap.close();
    this.pendingFullBitmaps.clear();
    for (const entry of this.cache.values()) entry.thumb?.close();
    this.cache.clear();
    this.totalBytes = 0;
  }

  /** Destroy worker entirely (unmount). */
  destroy(): void {
    this.clear();
    this.decoder.terminate();
  }

  // ── Worker callbacks ────────────────────────────────────────────

  private handleBitmap(hash: string, bitmap: ImageBitmap, quality: ThumbnailDecodeQuality): void {
    if (this.destroyed) { bitmap.close(); return; }

    if (quality === 'full') {
      this.discardPendingFullBitmap(hash);
      this.pendingFullBitmaps.set(hash, bitmap);
      this.scheduleFullAdmission();
      return;
    }

    this.installBitmap(hash, bitmap, quality);
  }

  private installBitmap(
    hash: string,
    bitmap: ImageBitmap,
    quality: ThumbnailDecodeQuality,
  ): void {
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
    this.installedQuality.set(hash, quality);

    if (quality === 'thumbnail' && this.fullEligibleHashes.has(hash)) {
      // The next draw upgrades this tile to the original. Resetting the
      // fingerprint is necessary because its layout did not change.
      this.lastPlanFingerprint = -1;
    }

    if (!isUpgrade) this.onBitmapAvailable(hash);
    this.onDirty();
  }

  private scheduleFullAdmission(): void {
    if (this.fullAdmissionFrame != null || this.pendingFullBitmaps.size === 0) return;
    this.fullAdmissionFrame = this.scheduleFrame(() => {
      this.fullAdmissionFrame = null;
      const next = this.pendingFullBitmaps.entries().next().value as [string, ImageBitmap] | undefined;
      if (!next) return;
      const [hash, bitmap] = next;
      this.pendingFullBitmaps.delete(hash);
      if (this.destroyed) bitmap.close();
      else this.installBitmap(hash, bitmap, 'full');
      this.scheduleFullAdmission();
    });
  }

  private discardPendingFullBitmap(hash: string): void {
    const bitmap = this.pendingFullBitmaps.get(hash);
    if (!bitmap) return;
    bitmap.close();
    this.pendingFullBitmaps.delete(hash);
    if (this.pendingFullBitmaps.size === 0 && this.fullAdmissionFrame != null) {
      this.cancelFrame(this.fullAdmissionFrame);
      this.fullAdmissionFrame = null;
    }
  }

  private handleError(
    hash: string,
    quality: ThumbnailDecodeQuality,
    failure?: ThumbnailDecodeFailure,
  ): void {
    if (failure?.terminal) {
      console.warn('[grid] thumbnail decode exhausted retries', {
        hash,
        quality,
        ...failure,
      });
    }
    let entry = this.cache.get(hash);
    if (quality === 'full' && entry?.thumb) return;
    if (!entry) {
      entry = { thumb: null, state: 'idle', lastAccessed: 0, bytes: 0 };
      this.cache.set(hash, entry);
    }
    entry.state = 'error';
    this.onDirty();
  }
}
