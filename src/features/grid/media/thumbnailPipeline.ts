import { api } from '#desktop/api';
import { mediaThumbnailUrl } from '../../../shared/lib/mediaUrl';
import {
  clampThumbnailDecodeSide,
  sortThumbnailQueue,
  THUMBNAIL_PIPELINE_MAX_ACTIVE_IDLE,
  THUMBNAIL_PIPELINE_MAX_ACTIVE_SCROLL,
  THUMBNAIL_PIPELINE_MAX_ENTRIES,
  THUMBNAIL_PIPELINE_STALL_MS,
} from './thumbnailPipelinePolicy';
import type {
  ThumbnailPipelineEntry,
  ThumbnailPipelineStats,
  ThumbnailQueueItem,
} from './thumbnailPipelineTypes';

export type {
  ThumbnailPipelineEntry,
  ThumbnailPipelineStats,
} from './thumbnailPipelineTypes';

export class ThumbnailPipeline {
  private cache = new Map<string, ThumbnailPipelineEntry>();
  private queue: ThumbnailQueueItem[] = [];
  private activeLoads = 0;
  private accessCounter = 0;
  private viewportTop = 0;
  private viewportHeight = 0;
  private scrolling = false;
  private destroyed = false;
  private loadedHashes = new Set<string>();
  private inFlight = new Map<string, AbortController>();

  constructor(private readonly onDirty: () => void) {}

  setScrolling(active: boolean): void {
    this.scrolling = active;
    this.pump();
  }

  setViewport(scrollTop: number, viewportHeight: number): void {
    this.viewportTop = scrollTop;
    this.viewportHeight = viewportHeight;
    this.sortQueue();
    this.pump();
  }

  resetFrameBudget(): void {
    // Legacy no-op: canvas draw loop still calls this once per frame.
  }

  get(hash: string): ThumbnailPipelineEntry | null {
    const entry = this.cache.get(hash);
    if (!entry) return null;
    entry.lastAccessed = ++this.accessCounter;
    return entry;
  }

  ensure(
    hash: string,
    mime: string,
    tileWidth: number,
    tileHeight: number,
    y?: number,
  ): void {
    if (this.destroyed) return;

    let entry = this.cache.get(hash);
    if (!entry) {
      entry = {
        thumb: null,
        quality: 'none',
        thumbRequested: false,
        thumbLoading: false,
        thumbRequestedAt: 0,
        createdAt: performance.now(),
        lastAccessed: ++this.accessCounter,
        revealStartedAt: 0,
        animateIn: false,
        error: false,
        repairQueued: false,
      };
      this.cache.set(hash, entry);
      this.pruneCache();
    } else {
      entry.lastAccessed = ++this.accessCounter;
    }

    if (entry.thumb) return;

    const now = performance.now();
    if (
      entry.thumbRequested &&
      entry.thumbRequestedAt > 0 &&
      now - entry.thumbRequestedAt > THUMBNAIL_PIPELINE_STALL_MS
    ) {
      entry.thumbRequested = false;
      entry.thumbLoading = false;
      entry.thumbRequestedAt = 0;
      this.inFlight.get(hash)?.abort();
      this.inFlight.delete(hash);
    }

    if (entry.thumbRequested) return;

    entry.thumbRequested = true;
    entry.thumbLoading = true;
    entry.thumbRequestedAt = now;
    entry.error = false;
    this.queue.push({
      hash,
      url: mediaThumbnailUrl(hash),
      y: y ?? this.viewportTop + this.viewportHeight / 2,
      mime,
      targetW: tileWidth,
      targetH: tileHeight,
    });
    this.sortQueue();
    this.pump();
  }

  cancelOutsideWindow(top: number, bottom: number): void {
    for (let i = this.queue.length - 1; i >= 0; i--) {
      const item = this.queue[i];
      if (item.y >= top && item.y <= bottom) continue;
      this.queue.splice(i, 1);
      const entry = this.cache.get(item.hash);
      if (!entry) continue;
      entry.thumbRequested = false;
      entry.thumbLoading = false;
      entry.thumbRequestedAt = 0;
      if (!entry.thumb) entry.error = false;
    }

    for (const [hash, controller] of this.inFlight) {
      const entry = this.cache.get(hash);
      if (!entry?.thumbLoading) continue;
      const itemStillQueued = this.queue.some((item) => item.hash === hash);
      if (itemStillQueued) continue;
      controller.abort();
      this.inFlight.delete(hash);
      entry.thumbRequested = false;
      entry.thumbLoading = false;
      entry.thumbRequestedAt = 0;
    }
  }

  getStats(): ThumbnailPipelineStats {
    return {
      queueDepth: this.queue.length,
      activeLoads: this.activeLoads,
      pendingThumbs: this.queue.length,
      cacheSize: this.cache.size,
      diskSpeed: 'normal',
    };
  }

  destroy(): void {
    this.destroyed = true;
    for (const controller of this.inFlight.values()) controller.abort();
    this.inFlight.clear();
    this.queue.length = 0;
    for (const entry of this.cache.values()) {
      entry.thumb?.close();
    }
    this.cache.clear();
  }

  private sortQueue(): void {
    sortThumbnailQueue(this.queue, this.viewportTop, this.viewportHeight);
  }

  private pump(): void {
    if (this.destroyed) return;
    const maxActive = this.scrolling
      ? THUMBNAIL_PIPELINE_MAX_ACTIVE_SCROLL
      : THUMBNAIL_PIPELINE_MAX_ACTIVE_IDLE;
    while (this.activeLoads < maxActive && this.queue.length > 0) {
      const next = this.queue.shift();
      if (!next) break;
      void this.loadThumb(next);
    }
  }

  private async loadThumb(item: ThumbnailQueueItem): Promise<void> {
    const entry = this.cache.get(item.hash);
    if (!entry || entry.thumb) return;

    const controller = new AbortController();
    this.inFlight.set(item.hash, controller);
    this.activeLoads += 1;
    try {
      const response = await fetch(item.url, { signal: controller.signal });
      if (!response.ok) throw new Error(`thumbnail fetch failed: ${response.status}`);
      const blob = await response.blob();
      const decodeMax = clampThumbnailDecodeSide(item.mime, this.scrolling);
      const bitmap = await createImageBitmap(blob, {
        resizeWidth: Math.max(1, Math.min(decodeMax, Math.round(item.targetW))),
        resizeHeight: Math.max(1, Math.min(decodeMax, Math.round(item.targetH))),
        resizeQuality: 'high',
      });
      this.applyBitmap(item.hash, bitmap);
    } catch {
      if (controller.signal.aborted) return;
      const current = this.cache.get(item.hash);
      if (current && !current.repairQueued) {
        current.repairQueued = true;
        void api.file.ensureThumbnail(item.hash)
          .catch(() => {})
          .finally(() => {
            const retryEntry = this.cache.get(item.hash);
            if (!retryEntry || retryEntry.thumb) return;
            retryEntry.repairQueued = false;
            retryEntry.thumbRequested = false;
            retryEntry.thumbLoading = false;
            retryEntry.thumbRequestedAt = 0;
            this.ensure(item.hash, item.mime, item.targetW, item.targetH, item.y);
          });
      } else if (current) {
        current.error = true;
        current.thumbRequested = false;
        current.thumbLoading = false;
        current.thumbRequestedAt = 0;
        this.onDirty();
      }
    } finally {
      this.inFlight.delete(item.hash);
      this.activeLoads = Math.max(0, this.activeLoads - 1);
      this.pump();
    }
  }

  private applyBitmap(hash: string, bitmap: ImageBitmap): void {
    const entry = this.cache.get(hash);
    if (!entry) {
      bitmap.close();
      return;
    }
    entry.thumb?.close();
    entry.thumb = bitmap;
    entry.quality = 'thumb';
    entry.thumbRequested = false;
    entry.thumbLoading = false;
    entry.thumbRequestedAt = 0;
    entry.error = false;
    entry.animateIn = !this.loadedHashes.has(hash);
    entry.revealStartedAt = performance.now();
    this.loadedHashes.add(hash);
    this.onDirty();
  }

  private pruneCache(): void {
    if (this.cache.size <= THUMBNAIL_PIPELINE_MAX_ENTRIES) return;
    const evictable = Array.from(this.cache.entries())
      .filter(([, entry]) => !entry.thumbLoading)
      .sort((a, b) => a[1].lastAccessed - b[1].lastAccessed);
    while (this.cache.size > THUMBNAIL_PIPELINE_MAX_ENTRIES && evictable.length > 0) {
      const [hash, entry] = evictable.shift()!;
      entry.thumb?.close();
      this.cache.delete(hash);
      this.loadedHashes.delete(hash);
      this.inFlight.get(hash)?.abort();
      this.inFlight.delete(hash);
    }
  }
}
