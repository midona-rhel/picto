import { api } from '#desktop/api';
import { mediaThumbnailUrl } from '../mediaUrl';
import {
  clampThumbnailDecodeSide,
  THUMBNAIL_PIPELINE_MAX_ACTIVE_IDLE,
  THUMBNAIL_PIPELINE_MAX_ACTIVE_SCROLL,
  THUMBNAIL_PIPELINE_MAX_ENTRIES,
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
  private scrolling = false;
  private destroyed = false;
  private loadedHashes = new Set<string>();
  private inFlight = new Map<string, AbortController>();

  constructor(private readonly onDirty: () => void) {}

  setScrolling(active: boolean): void {
    this.scrolling = active;
    this.pump();
  }

  get(hash: string): ThumbnailPipelineEntry | null {
    const entry = this.cache.get(hash);
    if (!entry) return null;
    this.touch(entry);
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

    const entry = this.getOrCreateEntry(hash);
    if (entry.thumb || entry.state === 'queued' || entry.state === 'loading') return;

    this.markQueued(entry);
    this.queue.push({
      hash,
      url: mediaThumbnailUrl(hash),
      y: y ?? 0,
      mime,
      targetW: tileWidth,
      targetH: tileHeight,
    });
    this.pump();
  }

  cancelOutsideWindow(top: number, bottom: number): void {
    for (let i = this.queue.length - 1; i >= 0; i--) {
      const item = this.queue[i];
      if (item.y >= top && item.y <= bottom) continue;
      this.queue.splice(i, 1);
      const entry = this.cache.get(item.hash);
      if (!entry) continue;
      if (!entry.thumb) this.resetEntry(entry);
    }

    for (const [hash, controller] of this.inFlight) {
      const entry = this.cache.get(hash);
      if (!entry || entry.state !== 'loading') continue;
      const itemStillQueued = this.queue.some((item) => item.hash === hash);
      if (itemStillQueued) continue;
      controller.abort();
      this.inFlight.delete(hash);
      this.resetEntry(entry);
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
    this.markLoading(entry);
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
      if (current && !current.retryQueued) {
        this.queueRepairRetry(current, item);
      } else if (current) {
        this.markError(current);
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
    entry.state = 'shown';
    entry.animateIn = !this.loadedHashes.has(hash);
    entry.revealStartedAt = performance.now();
    entry.retryQueued = false;
    this.loadedHashes.add(hash);
    this.onDirty();
  }

  private pruneCache(): void {
    if (this.cache.size <= THUMBNAIL_PIPELINE_MAX_ENTRIES) return;
    const target = THUMBNAIL_PIPELINE_MAX_ENTRIES - 100; // evict a batch to avoid per-insert sorting
    const entries = Array.from(this.cache.entries());
    entries.sort((a, b) => a[1].lastAccessed - b[1].lastAccessed);
    for (const [hash, entry] of entries) {
      if (this.cache.size <= target) break;
      if (entry.state === 'queued' || entry.state === 'loading') continue;
      entry.thumb?.close();
      this.cache.delete(hash);
      this.loadedHashes.delete(hash);
      this.inFlight.get(hash)?.abort();
      this.inFlight.delete(hash);
    }
  }

  private getOrCreateEntry(hash: string): ThumbnailPipelineEntry {
    let entry = this.cache.get(hash);
    if (entry) {
      this.touch(entry);
      return entry;
    }

    entry = {
      thumb: null,
      state: 'idle',
      lastAccessed: ++this.accessCounter,
      revealStartedAt: 0,
      animateIn: false,
      retryQueued: false,
    };
    this.cache.set(hash, entry);
    this.pruneCache();
    return entry;
  }

  private touch(entry: ThumbnailPipelineEntry): void {
    entry.lastAccessed = ++this.accessCounter;
  }

  private markQueued(entry: ThumbnailPipelineEntry): void {
    entry.state = 'queued';
  }

  private markLoading(entry: ThumbnailPipelineEntry): void {
    entry.state = 'loading';
  }

  private markError(entry: ThumbnailPipelineEntry): void {
    entry.state = 'error';
  }

  private resetEntry(entry: ThumbnailPipelineEntry): void {
    entry.state = 'idle';
  }

  private queueRepairRetry(entry: ThumbnailPipelineEntry, item: ThumbnailQueueItem): void {
    entry.retryQueued = true;
    void api.file.ensureThumbnail(item.hash)
      .catch(() => {})
      .finally(() => {
        const retryEntry = this.cache.get(item.hash);
        if (!retryEntry || retryEntry.thumb) return;
        retryEntry.retryQueued = false;
        this.resetEntry(retryEntry);
        this.ensure(item.hash, item.mime, item.targetW, item.targetH, item.y);
      });
  }
}
