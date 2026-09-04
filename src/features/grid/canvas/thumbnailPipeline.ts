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
  FULL_QUALITY_VIEWPORT_DWELL_MS,
  THUMBNAIL_VIEWPORT_DWELL_MS,
} from './thumbnailTiming';
import {
  computePlanFingerprint,
  fullQualityDecodeSize,
  shouldLoadFullQualityOriginal,
  sortPlanTilesByViewportDistance,
  type PlanTile,
} from './thumbnailPlan';

export type { PlanTile } from './thumbnailPlan';

export type { ThumbnailPipelineEntry } from './thumbnailPipelineTypes';

/** Prepared thumbnails fit inside a 512px square. Promote before the grid upscales them. */
const THUMBNAIL_LONG_EDGE_PX = 512;
export { FULL_QUALITY_VIEWPORT_DWELL_MS, THUMBNAIL_VIEWPORT_DWELL_MS } from './thumbnailTiming';

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
  private activeHashes = new Set<string>();
  private readonly pendingFullBitmaps = new Map<string, ImageBitmap>();
  private fullAdmissionFrame: number | null = null;
  private readonly visibleSince = new Map<string, number>();
  private readonly viewportHashes = new Set<string>();
  private readonly loadTiles: PlanTile[] = [];
  private dwellTimer: number | null = null;

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
    private readonly scheduleDelay: (callback: () => void, delayMs: number) => number =
      (callback, delayMs) => window.setTimeout(callback, delayMs),
    private readonly cancelDelay: (handle: number) => void =
      (handle) => window.clearTimeout(handle),
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

  /** Build decode work only after a tile remains in the actual viewport. */
  updatePlan(
    tiles: PlanTile[],
    viewportCenterY: number,
    devicePixelRatio = 1,
    now = performance.now(),
  ): void {
    if (this.destroyed) return;

    if (this.dwellTimer != null) {
      this.cancelDelay(this.dwellTimer);
      this.dwellTimer = null;
    }

    const viewportHashes = this.viewportHashes;
    const loadTiles = this.loadTiles;
    viewportHashes.clear();
    loadTiles.length = 0;
    let nextWakeMs = Number.POSITIVE_INFINITY;
    for (const tile of tiles) {
      if (tile.inViewport === false) continue;
      viewportHashes.add(tile.fileHash);
      let visibleAt = this.visibleSince.get(tile.fileHash);
      if (visibleAt == null) {
        visibleAt = now;
        this.visibleSince.set(tile.fileHash, visibleAt);
      }

      const dwellMs = Math.max(0, now - visibleAt);
      const entry = this.cache.get(tile.fileHash);
      const hasBitmap = Boolean(entry?.thumb);
      tile.fullQualityEligible = true;
      const needsFull = shouldLoadFullQualityOriginal(tile, THUMBNAIL_LONG_EDGE_PX, devicePixelRatio);

      if (!hasBitmap) {
        if (dwellMs >= THUMBNAIL_VIEWPORT_DWELL_MS) {
          tile.fullQualityEligible = false;
          loadTiles.push(tile);
        } else {
          nextWakeMs = Math.min(nextWakeMs, THUMBNAIL_VIEWPORT_DWELL_MS - dwellMs);
        }
        if (needsFull && dwellMs < FULL_QUALITY_VIEWPORT_DWELL_MS) {
          nextWakeMs = Math.min(nextWakeMs, FULL_QUALITY_VIEWPORT_DWELL_MS - dwellMs);
        }
        continue;
      }

      if (dwellMs < THUMBNAIL_VIEWPORT_DWELL_MS) {
        nextWakeMs = Math.min(nextWakeMs, THUMBNAIL_VIEWPORT_DWELL_MS - dwellMs);
      }
      if (!needsFull) continue;
      if (dwellMs < FULL_QUALITY_VIEWPORT_DWELL_MS) {
        nextWakeMs = Math.min(nextWakeMs, FULL_QUALITY_VIEWPORT_DWELL_MS - dwellMs);
        continue;
      }

      const decodeSize = fullQualityDecodeSize(tile, devicePixelRatio);
      const hasRequiredFull = entry?.quality === 'full'
        && (entry.thumb?.width ?? 0) >= decodeSize.width
        && (entry.thumb?.height ?? 0) >= decodeSize.height;
      if (!hasRequiredFull) {
        tile.fullQualityEligible = true;
        loadTiles.push(tile);
      }
    }

    for (const hash of this.visibleSince.keys()) {
      if (!viewportHashes.has(hash)) this.visibleSince.delete(hash);
    }
    if (Number.isFinite(nextWakeMs)) {
      this.dwellTimer = this.scheduleDelay(() => {
        this.dwellTimer = null;
        this.onDirty();
      }, Math.max(1, Math.ceil(nextWakeMs)));
    }

    this.activeHashes.clear();
    for (const tile of tiles) this.activeHashes.add(tile.fileHash);

    const fingerprint = computePlanFingerprint(loadTiles, THUMBNAIL_LONG_EDGE_PX, devicePixelRatio);
    if (fingerprint === this.lastPlanFingerprint) return;
    this.lastPlanFingerprint = fingerprint;

    // Plan-entry order is the worker's fetch priority.
    sortPlanTilesByViewportDistance(loadTiles, viewportCenterY);

    const buf = this.planBuffer;
    buf.length = loadTiles.length;
    for (let i = 0; i < loadTiles.length; i++) {
      const tile = loadTiles[i];
      const needsFull = shouldLoadFullQualityOriginal(tile, THUMBNAIL_LONG_EDGE_PX, devicePixelRatio);
      const decodeSize = needsFull ? fullQualityDecodeSize(tile, devicePixelRatio) : null;
      const revision = this.revisions.get(tile.fileHash) ?? 0;
      const url = needsFull
        ? mediaFileUrl(tile.fileHash, tile.mime)
        : `${mediaThumbnailUrl(tile.fileHash)}?v=${revision}`;
      const quality: ThumbnailDecodeQuality = needsFull ? 'full' : 'thumbnail';
      if (!needsFull) this.discardPendingFullBitmap(tile.fileHash);
      if (buf[i]) {
        buf[i].fileHash = tile.fileHash;
        buf[i].url = url;
        buf[i].quality = quality;
        buf[i].resizeWidth = decodeSize?.width;
        buf[i].resizeHeight = decodeSize?.height;
      } else {
        buf[i] = {
          fileHash: tile.fileHash,
          url,
          quality,
          resizeWidth: decodeSize?.width,
          resizeHeight: decodeSize?.height,
        };
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
        entry.quality = null;
      }
      this.cache.delete(hash);
    }
  }

  /** Tear down — scope change or unmount. */
  clear(): void {
    this.destroyed = true;
    this.lastPlanFingerprint = -1;
    this.activeHashes.clear();
    this.decoder.clear();
    if (this.fullAdmissionFrame != null) {
      this.cancelFrame(this.fullAdmissionFrame);
      this.fullAdmissionFrame = null;
    }
    for (const bitmap of this.pendingFullBitmaps.values()) bitmap.close();
    this.pendingFullBitmaps.clear();
    this.visibleSince.clear();
    this.viewportHashes.clear();
    this.loadTiles.length = 0;
    if (this.dwellTimer != null) {
      this.cancelDelay(this.dwellTimer);
      this.dwellTimer = null;
    }
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
    if (this.destroyed || !this.activeHashes.has(hash)) { bitmap.close(); return; }

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
      entry = { thumb: null, state: 'idle', lastAccessed: 0, bytes: 0, quality: null };
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
    entry.quality = quality;

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
      if (this.destroyed || !this.activeHashes.has(hash)) bitmap.close();
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
    if (this.destroyed || !this.activeHashes.has(hash)) return;
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
      entry = { thumb: null, state: 'idle', lastAccessed: 0, bytes: 0, quality: null };
      this.cache.set(hash, entry);
    }
    entry.state = 'error';
    this.onDirty();
  }
}
