/**
 * ImageAtlas — Canvas-optimized image loading, decoding, and LRU caching.
 *
 * Manages thumbnail and blurhash bitmaps for the CanvasGrid renderer.
 * Uses createImageBitmap() for zero-main-thread-jank loading.
 * Priority queue sorts by distance to viewport center for fast scroll.
 */

import { mediaThumbnailUrl, mediaFileUrl } from '../../lib/mediaUrl';
import BlurhashDecodeWorker from './blurhashDecodeWorker?worker';
import { api } from '#desktop/api';
import {
  enqueueMediaQosTask,
  getMediaQosStats,
  type MediaQosTaskHandle,
} from './mediaQosScheduler';

export interface AtlasEntry {
  thumb: ImageBitmap | null;
  blurhash: ImageBitmap | null;
  quality: 'none' | 'thumb' | 'full';
  thumbRequested: boolean;
  thumbLoading: boolean;
  thumbRequestedAt: number;
  fullRequested: boolean;
  fullLoading: boolean;
  blurhashFadeStartAt: number;
  blurhashFadeEndAt: number;
  createdAt: number;
  lastAccessed: number; // monotonic counter for LRU
}

export interface AtlasStats {
  queueDepth: number;
  activeLoads: number;
  pendingBlurhash: number;
  cacheSize: number;
  diskSpeed: 'normal' | 'fast';
}

interface QueueItem {
  hash: string;
  url: string;
  y: number; // tile center Y for viewport-priority sorting
  mime: string;
  targetW: number;
  targetH: number;
  kind: 'thumb' | 'full';
  readyAt: number;
}

interface BlurhashDecodeResponse {
  id: number;
  hash: string;
  width: number;
  height: number;
  pixels: ArrayBuffer;
}

const MAX_ENTRIES = 2000;
const MAX_CONCURRENT = 6;
// Keep grid reads thumbnail-first longer; full originals are reserved for very large tiles/detail.
const THUMB_MAX_SIDE = 900;
const MAX_BLURHASH_PER_FRAME_IDLE = 6;
const MAX_BLURHASH_PER_FRAME_SCROLL = 2;
const BLURHASH_HOLD_MS = 24;
const FULL_QUALITY_DELAY_MS = 220;
const FULL_QUALITY_HEAVY_DELAY_MS = 700;
const FULL_QUALITY_MIN_SIDE = 1200;
const FULL_HEAVY_DECODE_MAX_SIDE = 1500;
const SCROLL_MAX_CONCURRENT = 3;
const SCROLL_MAX_HEAVY_CONCURRENT = 1;
const IDLE_MAX_HEAVY_CONCURRENT = 2;
const THUMB_DECODE_MAX_SIDE_SCROLL = 256;
const THUMB_DECODE_MAX_SIDE_IDLE = 384;
const WEBP_THUMB_DECODE_MAX_SIDE_SCROLL = 192;
const WEBP_THUMB_DECODE_MAX_SIDE_IDLE = 320;
const AVIF_THUMB_DECODE_MAX_SIDE_SCROLL = 160;
const AVIF_THUMB_DECODE_MAX_SIDE_IDLE = 256;
const MAX_REPAIR_IN_FLIGHT = 2;
const BLURHASH_REVEAL_HOLD_MS = 100;
const BLURHASH_CROSSFADE_MS = 150;

function safeAspectRatio(value: number): number {
  if (!Number.isFinite(value) || value <= 0) return 1.5;
  return Math.min(8, Math.max(0.125, value));
}

export class ImageAtlas {
  private cache = new Map<string, AtlasEntry>();
  private accessCounter = 0;
  private activeLoads = 0;
  private activeHeavyLoads = 0;
  private loadQueue: QueueItem[] = [];
  private scheduledLoads = new Map<string, { handle: MediaQosTaskHandle; heavy: boolean }>();
  private queueWakeTimer: number | null = null;
  private blurhashQueue: Array<{ hash: string; str: string; ar: number }> = [];
  private blurhashDecodeCount = 0; // Resets each frame via resetFrameBudget()
  private blurhashWorker: Worker | null = null;
  private blurhashRequestSeq = 0;
  private pendingBlurhashByHash = new Set<string>();
  private repairingThumbHashes = new Set<string>();
  private repairQueue: string[] = [];
  private repairQueueSet = new Set<string>();
  private repairInFlight = 0;
  private perfLoadCount = 0;
  private perfLoadMsTotal = 0;
  private diskSpeed: 'normal' | 'fast' = 'normal';
  private scrolling = false;
  private onDirty: () => void;
  private destroyed = false;

  // Viewport state for priority sorting
  private viewportTop = 0;
  private viewportHeight = 0;

  constructor(onDirty: () => void) {
    this.onDirty = onDirty;
    this.initBlurhashWorker();
  }

  setScrolling(active: boolean): void {
    this.scrolling = active;
  }

  /** Update the current viewport for priority sorting. */
  setViewport(scrollTop: number, viewportHeight: number): void {
    this.viewportTop = scrollTop;
    this.viewportHeight = viewportHeight;
  }

  /** Cancel queued loads outside the given window and prune stale loading entries. */
  cancelOutsideWindow(top: number, bottom: number): void {
    for (let i = this.loadQueue.length - 1; i >= 0; i--) {
      const item = this.loadQueue[i];
      if (item.y < top || item.y > bottom) {
        this.loadQueue.splice(i, 1);
        // Update request flags for dropped queued work.
        const entry = this.cache.get(item.hash);
        if (entry) {
          if (item.kind === 'thumb') {
            entry.thumbRequested = false;
            entry.thumbLoading = false;
            entry.thumbRequestedAt = 0;
          } else {
            entry.fullRequested = false;
            entry.fullLoading = false;
          }
        }
        // Prune entry if no image is available and nothing is actively loading.
        if (
          entry &&
          !entry.thumb &&
          !entry.thumbLoading &&
          !entry.fullLoading
        ) {
          entry.blurhash?.close();
          this.cache.delete(item.hash);
        }
      }
    }
    if (this.loadQueue.length === 0 && this.queueWakeTimer != null) {
      window.clearTimeout(this.queueWakeTimer);
      this.queueWakeTimer = null;
    }
  }

  /** Get cached bitmaps for a hash. Returns null entry fields if not yet loaded. */
  get(hash: string): AtlasEntry | null {
    const entry = this.cache.get(hash);
    if (entry) {
      entry.lastAccessed = ++this.accessCounter;
    }
    return entry ?? null;
  }

  /**
   * Ensure an image is loading or loaded.
   * Call this for every visible tile each frame — it's a no-op if already cached/loading.
   * @param y Optional tile center Y for viewport-priority sorting. Defaults to viewport center.
   */
  ensure(
    hash: string,
    mime: string,
    tileWidth: number,
    tileHeight: number,
    blurhashStr?: string | null,
    aspectRatio?: number,
    y?: number,
    allowBlurhash = true,
  ): void {
    if (this.destroyed) return;

    let justCreated = false;
    let entry = this.cache.get(hash);
    if (entry) {
      entry.lastAccessed = ++this.accessCounter;
    } else {
      justCreated = true;
      // Create entry
      entry = {
        thumb: null,
        blurhash: null,
        quality: 'none',
        thumbRequested: false,
        thumbLoading: false,
        thumbRequestedAt: 0,
        fullRequested: false,
        fullLoading: false,
        blurhashFadeStartAt: 0,
        blurhashFadeEndAt: 0,
        createdAt: performance.now(),
        lastAccessed: ++this.accessCounter,
      };
      this.cache.set(hash, entry);
    }

    // Decode blurhash for both new and existing entries. This fixes the case where
    // a tile first appears during scroll (allowBlurhash=false) and never decodes later.
    if (!entry.blurhash && blurhashStr && allowBlurhash && !this.pendingBlurhashByHash.has(hash)) {
      const frameBudget = this.scrolling
        ? MAX_BLURHASH_PER_FRAME_SCROLL
        : MAX_BLURHASH_PER_FRAME_IDLE;
      if (this.blurhashDecodeCount < frameBudget) {
        this.blurhashDecodeCount++;
        this.decodeBlurhash(hash, blurhashStr, safeAspectRatio(aspectRatio ?? 1));
      } else if (!this.blurhashQueue.some((item) => item.hash === hash)) {
        this.blurhashQueue.push({ hash, str: blurhashStr, ar: safeAspectRatio(aspectRatio ?? 1) });
      }
    }

    // Backfill missing blurhash/thumbnail when metadata has no blurhash.
    // Do this even while scrolling so entries self-heal quickly.
    if (!blurhashStr && (justCreated || !entry.blurhash)) {
      this.enqueueThumbnailRepair(hash);
    }

    const now = performance.now();
    const tileY = y ?? (this.viewportTop + this.viewportHeight / 2);
    const maxDisplaySide = Math.max(tileWidth, tileHeight);

    // Recovery path: if thumbnail request appears stalled, reset flags and retry.
    if (!entry.thumb && entry.thumbRequested && entry.thumbRequestedAt > 0) {
      const elapsed = now - entry.thumbRequestedAt;
      const queuedStall = !entry.thumbLoading && elapsed > 2500;
      const loadingStall = entry.thumbLoading && elapsed > 5000;
      if (queuedStall || loadingStall) {
        entry.thumbRequested = false;
        entry.thumbLoading = false;
        entry.thumbRequestedAt = 0;
      }
    }

    // Stage 1: thumbnail (optionally delayed so blurhash has a chance to render first).
    // Queue immediately with a timed readyAt so we don't depend on a future redraw to start loading.
    const holdMs = blurhashStr && allowBlurhash ? BLURHASH_HOLD_MS : 0;
    if (!this.repairingThumbHashes.has(hash) && !entry.thumbRequested) {
      entry.thumbRequested = true;
      entry.thumbLoading = true;
      entry.thumbRequestedAt = now;
      this.enqueue(
        hash,
        mediaThumbnailUrl(hash),
        tileY,
        'thumb',
        mime,
        tileWidth,
        tileHeight,
        holdMs,
      );
    }

    // Stage 2: delayed promotion to full quality for very large tiles.
    const heavyFullMime = mime === 'image/webp' || mime === 'image/avif';
    const fullDelayMs = heavyFullMime ? FULL_QUALITY_HEAVY_DELAY_MS : FULL_QUALITY_DELAY_MS;
    if (
      !this.scrolling &&
      maxDisplaySide > THUMB_MAX_SIDE &&
      maxDisplaySide >= FULL_QUALITY_MIN_SIDE &&
      entry.quality === 'thumb' &&
      !entry.fullRequested &&
      now - entry.createdAt >= fullDelayMs
    ) {
      entry.fullRequested = true;
      entry.fullLoading = true;
      this.enqueue(hash, mediaFileUrl(hash, mime), tileY, 'full', mime, tileWidth, tileHeight, 0);
    }

    // Evict if over capacity
    if (this.cache.size > MAX_ENTRIES) {
      this.evict();
    }
  }

  /** Cancel all pending loads and mark as destroyed. */
  destroy(): void {
    this.destroyed = true;
    this.loadQueue.length = 0;
    for (const [, scheduled] of this.scheduledLoads) {
      scheduled.handle.cancel();
    }
    this.scheduledLoads.clear();
    this.activeLoads = 0;
    this.activeHeavyLoads = 0;
    this.repairQueue.length = 0;
    this.repairQueueSet.clear();
    this.repairingThumbHashes.clear();
    this.repairInFlight = 0;
    if (this.queueWakeTimer != null) {
      window.clearTimeout(this.queueWakeTimer);
      this.queueWakeTimer = null;
    }
    if (this.blurhashWorker) {
      this.blurhashWorker.terminate();
      this.blurhashWorker = null;
    }
    this.pendingBlurhashByHash.clear();
    // Close all ImageBitmaps to free GPU memory
    for (const entry of this.cache.values()) {
      entry.thumb?.close();
      entry.blurhash?.close();
    }
    this.cache.clear();
  }

  /** Call at the start of each drawFrame to reset per-frame decode budget. */
  resetFrameBudget(): void {
    this.blurhashDecodeCount = 0;
    const frameBudget = this.scrolling
      ? MAX_BLURHASH_PER_FRAME_SCROLL
      : MAX_BLURHASH_PER_FRAME_IDLE;
    // Process queued blurhash decodes from previous frame
    if (this.blurhashQueue.length > 0) {
      const batch = this.blurhashQueue.splice(0, frameBudget);
      for (const item of batch) {
        if (this.cache.has(item.hash)) {
          this.decodeBlurhash(item.hash, item.str, item.ar);
        }
      }
      // If more remain, trigger another draw next frame
      if (this.blurhashQueue.length > 0) {
        this.onDirty();
      }
    }
  }

  /** Evict cached bitmaps for given hashes so they will be re-fetched on next draw. */
  invalidateHashes(hashes: string[]): void {
    for (const hash of hashes) {
      const thumbTask = this.taskKey(hash, 'thumb');
      const fullTask = this.taskKey(hash, 'full');
      const thumbScheduled = this.scheduledLoads.get(thumbTask);
      if (thumbScheduled) {
        thumbScheduled.handle.cancel();
        this.completeScheduledLoad(thumbTask);
      }
      const fullScheduled = this.scheduledLoads.get(fullTask);
      if (fullScheduled) {
        fullScheduled.handle.cancel();
        this.completeScheduledLoad(fullTask);
      }
      const entry = this.cache.get(hash);
      if (entry) {
        entry.thumb?.close();
        entry.blurhash?.close();
        this.cache.delete(hash);
      }
    }
    this.onDirty();
  }

  /** Cancel pending loads (e.g. on layout change). Active loads will still complete. */
  cancelPending(): void {
    this.loadQueue.length = 0;
    if (this.queueWakeTimer != null) {
      window.clearTimeout(this.queueWakeTimer);
      this.queueWakeTimer = null;
    }
  }

  /** Current number of items waiting in the load queue. */
  getQueueDepth(): number {
    return this.loadQueue.length;
  }

  /** Current number of in-flight image loads. */
  getActiveLoads(): number {
    return this.activeLoads;
  }

  getStats(): AtlasStats {
    const qos = getMediaQosStats();
    return {
      queueDepth: this.loadQueue.length + qos.queuedByLane.visible,
      activeLoads: this.activeLoads,
      pendingBlurhash: this.pendingBlurhashByHash.size + this.blurhashQueue.length,
      cacheSize: this.cache.size,
      diskSpeed: this.diskSpeed,
    };
  }

  private taskKey(hash: string, kind: 'thumb' | 'full'): string {
    return `${hash}:${kind}`;
  }

  private completeScheduledLoad(taskKey: string): void {
    const scheduled = this.scheduledLoads.get(taskKey);
    if (!scheduled) return;
    this.scheduledLoads.delete(taskKey);
    this.activeLoads = Math.max(0, this.activeLoads - 1);
    if (scheduled.heavy) this.activeHeavyLoads = Math.max(0, this.activeHeavyLoads - 1);
  }

  private decodeBlurhash(hash: string, blurhashStr: string, aspectRatio: number): void {
    if (this.pendingBlurhashByHash.has(hash)) return;
    if (!this.blurhashWorker) {
      this.initBlurhashWorker();
    }
    if (!this.blurhashWorker) return;
    this.pendingBlurhashByHash.add(hash);
    this.blurhashRequestSeq += 1;
    this.blurhashWorker.postMessage({
      id: this.blurhashRequestSeq,
      hash,
      blurhash: blurhashStr,
      aspectRatio,
    });
  }

  private initBlurhashWorker(): void {
    if (this.blurhashWorker || this.destroyed) return;
    try {
      const worker = new BlurhashDecodeWorker();
      worker.onmessage = (event: MessageEvent<BlurhashDecodeResponse>) => {
        if (this.destroyed) return;
        const payload = event.data;
        this.pendingBlurhashByHash.delete(payload.hash);
        const entry = this.cache.get(payload.hash);
        if (!entry) return;
        const imageData = new ImageData(
          new Uint8ClampedArray(payload.pixels),
          payload.width,
          payload.height,
        );
        createImageBitmap(imageData).then((bitmap) => {
          if (this.destroyed) {
            bitmap.close();
            return;
          }
          const activeEntry = this.cache.get(payload.hash);
          if (!activeEntry) {
            bitmap.close();
            return;
          }
          activeEntry.blurhash?.close();
          activeEntry.blurhash = bitmap;
          this.onDirty();
        }).catch(() => {
          // Ignore bitmap failures; tile will use regular placeholder/thumb path.
        });
      };
      worker.onerror = () => {
        // Leave worker path disabled if runtime cannot host worker for any reason.
      };
      this.blurhashWorker = worker;
    } catch {
      // Ignore; we'll continue without worker blurhash placeholders.
    }
  }

  private enqueueThumbnailRepair(hash: string): void {
    if (this.destroyed) return;
    if (this.repairQueueSet.has(hash) || this.repairingThumbHashes.has(hash)) return;
    this.repairQueue.push(hash);
    this.repairQueueSet.add(hash);
    this.processThumbnailRepairs();
  }

  private processThumbnailRepairs(): void {
    if (this.destroyed) return;
    while (this.repairInFlight < MAX_REPAIR_IN_FLIGHT && this.repairQueue.length > 0) {
      const hash = this.repairQueue.shift();
      if (!hash) break;
      this.repairQueueSet.delete(hash);
      this.repairingThumbHashes.add(hash);
      this.repairInFlight++;

      api.file.ensureThumbnail(hash)
        .then((result) => {
          if (this.destroyed) return;
          const entry = this.cache.get(hash);
          if (!entry) return;

          // If blurhash was backfilled server-side, decode it for immediate placeholder usage.
          if (result?.blurhash && !entry.blurhash) {
            this.decodeBlurhash(hash, result.blurhash, 1);
          }

          // Allow normal thumb stage to retry now that storage has been repaired.
          if (result?.has_thumbnail) {
            entry.thumbRequested = false;
            entry.thumbLoading = false;
            entry.thumbRequestedAt = 0;
            this.onDirty();
          }
        })
        .catch(() => {
          // Ignore repair errors; normal loading path will continue retrying.
        })
        .finally(() => {
          this.repairingThumbHashes.delete(hash);
          this.repairInFlight = Math.max(0, this.repairInFlight - 1);
          this.processThumbnailRepairs();
        });
    }
  }

  private enqueue(
    hash: string,
    url: string,
    y: number,
    kind: 'thumb' | 'full',
    mime: string,
    targetW: number,
    targetH: number,
    delayFloorMs: number,
  ): void {
    const delayMs = this.calculateDelay(y) + Math.max(0, delayFloorMs);
    const readyAt = performance.now() + delayMs;
    for (let i = 0; i < this.loadQueue.length; i++) {
      const q = this.loadQueue[i];
      if (q.hash === hash && q.kind === kind) {
        q.y = y;
        q.mime = mime;
        q.targetW = Math.max(q.targetW, targetW);
        q.targetH = Math.max(q.targetH, targetH);
        q.readyAt = Math.min(q.readyAt, readyAt);
        this.scheduleQueueWake();
        return;
      }
    }
    this.loadQueue.push({ hash, url, y, kind, mime, targetW, targetH, readyAt });
    this.processQueue();
    this.scheduleQueueWake();
  }

  private startLoad(item: QueueItem): void {
    const { hash, url, kind, mime, targetW, targetH } = item;
    const taskKey = this.taskKey(hash, kind);
    if (this.scheduledLoads.has(taskKey)) return;
    const heavyDecode = mime === 'image/webp' || mime === 'image/avif';

    this.activeLoads++;
    if (heavyDecode) this.activeHeavyLoads++;

    const handle = enqueueMediaQosTask({
      lane: 'visible',
      priority: this.scrolling ? 10 : 0,
      heavy: heavyDecode,
      run: (signal) => {
        return new Promise<void>((resolve) => {
          const loadStart = performance.now();
          const img = new Image();
          img.decoding = 'async';
          let settled = false;

          const cleanup = () => {
            img.onload = null;
            img.onerror = null;
            signal.removeEventListener('abort', onAbort);
            img.src = '';
          };

          const finish = () => {
            if (settled) return;
            settled = true;
            cleanup();
            this.completeScheduledLoad(taskKey);
            this.processQueue();
            this.scheduleQueueWake();
            resolve();
          };

          const onAbort = () => {
            const entry = this.cache.get(hash);
            if (entry) this.markLoadDone(entry, kind);
            finish();
          };

          const decodeBitmap = async (): Promise<ImageBitmap> => {
            const decodeSize = this.computeDecodeSize(kind, mime, targetW, targetH);
            if (decodeSize) {
              try {
                return await createImageBitmap(img, {
                  resizeWidth: decodeSize.w,
                  resizeHeight: decodeSize.h,
                  resizeQuality: this.scrolling ? 'low' : 'medium',
                });
              } catch {
                // Fall through to full-size decode when resize options are unsupported.
              }
            }
            return createImageBitmap(img);
          };

          const applyDecodedBitmap = (bitmap: ImageBitmap) => {
            if (this.destroyed || signal.aborted) {
              bitmap.close();
              finish();
              return;
            }
            const entry = this.cache.get(hash);
            if (entry) {
              this.applyBitmap(entry, bitmap, kind);
              this.recordLoadTime(performance.now() - loadStart);
            } else {
              bitmap.close();
            }
            finish();
          };

          signal.addEventListener('abort', onAbort, { once: true });
          img.src = url;

          const decodeLoaded = () => {
            if (signal.aborted) {
              finish();
              return;
            }
            decodeBitmap()
              .then(applyDecodedBitmap)
              .catch(() => {
                if (!this.destroyed) {
                  const entry = this.cache.get(hash);
                  if (entry) this.markLoadDone(entry, kind);
                  if (kind === 'thumb') this.enqueueThumbnailRepair(hash);
                }
                finish();
              });
          };

          if (img.complete && img.naturalHeight > 0) {
            decodeLoaded();
            return;
          }

          img.onload = decodeLoaded;
          img.onerror = () => {
            if (!this.destroyed) {
              const entry = this.cache.get(hash);
              if (entry) this.markLoadDone(entry, kind);
              if (kind === 'thumb') this.enqueueThumbnailRepair(hash);
            }
            finish();
          };
        });
      },
    });
    this.scheduledLoads.set(taskKey, { handle, heavy: heavyDecode });
  }

  private markLoadDone(entry: AtlasEntry, kind: 'thumb' | 'full'): void {
    if (kind === 'thumb') {
      entry.thumbLoading = false;
      if (!entry.thumb) entry.thumbRequested = false;
      if (!entry.thumb) entry.thumbRequestedAt = 0;
      return;
    }
    entry.fullLoading = false;
    if (entry.quality !== 'full') entry.fullRequested = false;
  }

  private applyBitmap(entry: AtlasEntry, bitmap: ImageBitmap, kind: 'thumb' | 'full'): void {
    const now = performance.now();
    if (entry.quality === 'none' && entry.blurhash) {
      entry.blurhashFadeStartAt = now + BLURHASH_REVEAL_HOLD_MS;
      entry.blurhashFadeEndAt = entry.blurhashFadeStartAt + BLURHASH_CROSSFADE_MS;
    }

    if (kind === 'thumb') {
      entry.thumbLoading = false;
      entry.thumbRequested = true;
      entry.thumbRequestedAt = now;
      if (entry.quality !== 'full') {
        entry.thumb?.close();
        entry.thumb = bitmap;
        entry.quality = 'thumb';
        this.onDirty();
      } else {
        bitmap.close();
      }
      return;
    }
    entry.fullLoading = false;
    entry.fullRequested = true;
    entry.thumb?.close();
    entry.thumb = bitmap;
    entry.quality = 'full';
    this.onDirty();
  }

  private processQueue(): void {
    const maxConcurrent = this.scrolling ? SCROLL_MAX_CONCURRENT : MAX_CONCURRENT;
    const maxHeavyConcurrent = this.scrolling ? SCROLL_MAX_HEAVY_CONCURRENT : IDLE_MAX_HEAVY_CONCURRENT;
    const viewportCenter = this.viewportTop + this.viewportHeight / 2;
    while (this.activeLoads < maxConcurrent && this.loadQueue.length > 0) {
      const next = this.popNearestReadyToViewportCenter(viewportCenter, performance.now());
      if (!next) break;
      // Skip if entry was evicted while queued
      if (!this.cache.has(next.hash)) continue;
      const heavy = next.mime === 'image/webp' || next.mime === 'image/avif';
      if (heavy && this.activeHeavyLoads >= maxHeavyConcurrent) {
        // Avoid decode storms from many queued heavy thumbs while still allowing other formats through.
        next.readyAt = performance.now() + 20;
        this.loadQueue.push(next);
        const hasReadyNonWebp = this.loadQueue.some(
          (item) => item.readyAt <= performance.now() && item.mime !== 'image/webp' && item.mime !== 'image/avif',
        );
        if (!hasReadyNonWebp) break;
        continue;
      }
      this.startLoad(next);
    }
  }

  private popNearestReadyToViewportCenter(centerY: number, now: number): QueueItem | null {
    if (this.loadQueue.length === 0) return null;
    let bestIdx = -1;
    let bestDist = Number.POSITIVE_INFINITY;
    for (let i = 0; i < this.loadQueue.length; i++) {
      if (this.loadQueue[i].readyAt > now) continue;
      const dist = Math.abs(this.loadQueue[i].y - centerY);
      if (dist < bestDist) {
        bestDist = dist;
        bestIdx = i;
      }
    }
    if (bestIdx < 0) return null;
    const lastIdx = this.loadQueue.length - 1;
    const best = this.loadQueue[bestIdx];
    this.loadQueue[bestIdx] = this.loadQueue[lastIdx];
    this.loadQueue.pop();
    return best ?? null;
  }

  private calculateDelay(tileCenterY: number): number {
    if (this.viewportHeight <= 0) return 0;
    const center = this.viewportTop + this.viewportHeight / 2;
    const distance = Math.abs(tileCenterY - center);
    const nearDistance = this.viewportHeight * 0.55;
    const midDistance = this.viewportHeight * 1.1;
    if (distance <= nearDistance) return 0;
    if (this.diskSpeed === 'fast') {
      return distance <= midDistance ? 10 : 50;
    }
    return distance <= midDistance ? 30 : 100;
  }

  private recordLoadTime(ms: number): void {
    this.perfLoadCount++;
    this.perfLoadMsTotal += ms;
    if (this.perfLoadCount > 10) {
      const avg = this.perfLoadMsTotal / this.perfLoadCount;
      this.diskSpeed = avg < 200 ? 'fast' : 'normal';
      if (this.perfLoadCount > 100) {
        this.perfLoadCount = 0;
        this.perfLoadMsTotal = 0;
      }
    }
  }

  private scheduleQueueWake(): void {
    if (this.destroyed) return;
    const maxConcurrent = this.scrolling ? SCROLL_MAX_CONCURRENT : MAX_CONCURRENT;
    if (this.activeLoads >= maxConcurrent) return;
    if (this.queueWakeTimer != null) return;
    if (this.loadQueue.length === 0) return;
    let minReadyAt = Number.POSITIVE_INFINITY;
    for (let i = 0; i < this.loadQueue.length; i++) {
      if (this.loadQueue[i].readyAt < minReadyAt) {
        minReadyAt = this.loadQueue[i].readyAt;
      }
    }
    const delay = Math.max(0, minReadyAt - performance.now());
    this.queueWakeTimer = window.setTimeout(() => {
      this.queueWakeTimer = null;
      this.processQueue();
      this.scheduleQueueWake();
    }, delay);
  }

  private evict(): void {
    if (this.cache.size <= MAX_ENTRIES) return;

    // Collect entries sorted by lastAccessed (ascending = oldest first)
    const entries = Array.from(this.cache.entries());
    entries.sort((a, b) => a[1].lastAccessed - b[1].lastAccessed);

    const toRemove = this.cache.size - MAX_ENTRIES;
    for (let i = 0; i < toRemove && i < entries.length; i++) {
      const [key, entry] = entries[i];
      entry.thumb?.close();
      entry.blurhash?.close();
      this.cache.delete(key);
    }

    // Also remove from load queue
    const remaining = this.cache;
    for (let i = this.loadQueue.length - 1; i >= 0; i--) {
      if (!remaining.has(this.loadQueue[i].hash)) {
        this.loadQueue.splice(i, 1);
      }
    }
  }

  private computeDecodeSize(
    kind: 'thumb' | 'full',
    mime: string,
    targetW: number,
    targetH: number,
  ): { w: number; h: number } | null {
    const safeW = Math.max(1, Math.round(targetW));
    const safeH = Math.max(1, Math.round(targetH));
    const maxDisplaySide = Math.max(safeW, safeH);

    // Full-quality promotion is expensive for WEBP/AVIF at very large source sizes.
    // Decode close to display resolution instead of full source resolution.
    if (kind === 'full') {
      if (mime !== 'image/webp' && mime !== 'image/avif') return null;
      const desiredMax = Math.min(
        FULL_HEAVY_DECODE_MAX_SIDE,
        Math.max(640, Math.ceil(maxDisplaySide * 1.08)),
      );
      const scale = Math.min(1, desiredMax / maxDisplaySide);
      return {
        w: Math.max(1, Math.round(safeW * scale)),
        h: Math.max(1, Math.round(safeH * scale)),
      };
    }

    let cap = this.scrolling ? THUMB_DECODE_MAX_SIDE_SCROLL : THUMB_DECODE_MAX_SIDE_IDLE;
    if (mime === 'image/webp') {
      cap = this.scrolling ? WEBP_THUMB_DECODE_MAX_SIDE_SCROLL : WEBP_THUMB_DECODE_MAX_SIDE_IDLE;
    } else if (mime === 'image/avif') {
      cap = this.scrolling ? AVIF_THUMB_DECODE_MAX_SIDE_SCROLL : AVIF_THUMB_DECODE_MAX_SIDE_IDLE;
    }

    const desiredMax = Math.min(
      cap,
      Math.max(96, Math.ceil(maxDisplaySide * (this.scrolling ? 1.08 : 1.22))),
    );
    if (desiredMax >= maxDisplaySide) return null;

    const scale = desiredMax / maxDisplaySide;
    return {
      w: Math.max(1, Math.round(safeW * scale)),
      h: Math.max(1, Math.round(safeH * scale)),
    };
  }
}
