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
    private readonly scheduleFrame: (callback: FrameRequestCallback) => number = requestAnimationFrame,
    private readonly cancelFrame: (handle: number) => void = cancelAnimationFrame,
  ) {
    this.onDirty = onDirty;
    this.onBitmapAvailable = onBitmapAvailable;
    this.decoder = new ThumbnailDecodeClient(
      (hash, bitmap, quality) => this.handleBitmap(hash, bitmap, quality),
      (hash, quality) => this.handleError(hash, quality),
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
      const needsFull = shouldLoadFullQualityOriginal(t, FULL_QUALITY_THRESHOLD_PX);
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
    }
  }

  /** Tear down — scope change or unmount. */
  clear(): void {
    this.destroyed = true;
    this.lastPlanFingerprint = -1;
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

    this.installBitmap(hash, bitmap);
  }

  private installBitmap(hash: string, bitmap: ImageBitmap): void {
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

  private scheduleFullAdmission(): void {
    if (this.fullAdmissionFrame != null || this.pendingFullBitmaps.size === 0) return;
    this.fullAdmissionFrame = this.scheduleFrame(() => {
      this.fullAdmissionFrame = null;
      const next = this.pendingFullBitmaps.entries().next().value as [string, ImageBitmap] | undefined;
      if (!next) return;
      const [hash, bitmap] = next;
      this.pendingFullBitmaps.delete(hash);
      if (this.destroyed) bitmap.close();
      else this.installBitmap(hash, bitmap);
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

  private handleError(hash: string, quality: ThumbnailDecodeQuality): void {
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
