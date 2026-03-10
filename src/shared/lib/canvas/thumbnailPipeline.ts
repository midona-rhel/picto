import { api } from '#desktop/api';
import { mediaFileUrl, mediaThumbnailUrl } from '../mediaUrl';
import {
  THUMBNAIL_PIPELINE_FULL_QUALITY_THRESHOLD,
  THUMBNAIL_PIPELINE_MAX_ACTIVE_IDLE,
  THUMBNAIL_PIPELINE_MAX_ACTIVE_SCROLL,
  THUMBNAIL_PIPELINE_MAX_FULL_ACTIVE_IDLE,
  THUMBNAIL_PIPELINE_MAX_FULL_LONG_EDGE,
  THUMBNAIL_PIPELINE_MAX_FULL_ACTIVE_SCROLL,
  THUMBNAIL_PIPELINE_MAX_ENTRIES,
  THUMBNAIL_PIPELINE_SOURCE_EDGE,
} from './thumbnailPipelinePolicy';
import type {
  ThumbnailPipelineEntry,
  ThumbnailInFlightItem,
  ThumbnailPipelineStats,
  ThumbnailQueueItem,
  ThumbnailRequestPriority,
  ThumbnailSourceKind,
} from './thumbnailPipelineTypes';

export type {
  ThumbnailPipelineEntry,
  ThumbnailPipelineStats,
} from './thumbnailPipelineTypes';

interface EnsureThumbnailArgs {
  y?: number;
  drawWidth?: number;
  drawHeight?: number;
  mime?: string;
  sourceWidth?: number | null;
  sourceHeight?: number | null;
}

export class ThumbnailPipeline {
  private cache = new Map<string, ThumbnailPipelineEntry>();
  private queue: ThumbnailQueueItem[] = [];
  private activeLoads = 0;
  private activeFullLoads = 0;
  private accessCounter = 0;
  private scrolling = false;
  private destroyed = false;
  private loadedHashes = new Set<string>();
  private inFlight = new Map<string, ThumbnailInFlightItem>();

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

  ensure(hash: string, args: EnsureThumbnailArgs = {}): void {
    if (this.destroyed) return;

    const entry = this.getOrCreateEntry(hash);
    const request = buildRequest(hash, args);
    if (!request) return;

    if (entry.thumb) {
      if (!needsUpgrade(entry, request)) return;
    } else if (entry.state === 'queued' || entry.state === 'loading') {
      const active = this.inFlight.get(hash);
      if (active && !needsUpgradeState(active.sourceKind, active.requestedLongEdge, request.sourceKind, request.requestedLongEdge)) {
        return;
      }
      const queued = this.queue.find((item) => item.hash === hash);
      if (queued && !needsUpgradeState(queued.sourceKind, queued.requestedLongEdge, request.sourceKind, request.requestedLongEdge)) {
        return;
      }
    }

    this.markQueued(entry);
    this.queue = this.queue.filter((item) => item.hash !== hash);
    this.queue.push(request);
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

    for (const [hash, inFlight] of this.inFlight) {
      const entry = this.cache.get(hash);
      if (!entry || entry.state !== 'loading') continue;
      if (inFlight.y >= top && inFlight.y <= bottom) continue;
      inFlight.controller.abort();
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
    for (const inFlight of this.inFlight.values()) inFlight.controller.abort();
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
    const maxFullActive = this.scrolling
      ? THUMBNAIL_PIPELINE_MAX_FULL_ACTIVE_SCROLL
      : THUMBNAIL_PIPELINE_MAX_FULL_ACTIVE_IDLE;
    while (this.activeLoads < maxActive) {
      const nextIndex = this.selectNextQueueIndex(maxFullActive);
      if (nextIndex < 0) break;
      const [next] = this.queue.splice(nextIndex, 1);
      if (!next) break;
      void this.loadThumb(next);
    }
  }

  private selectNextQueueIndex(maxFullActive: number): number {
    let bestIndex = -1;
    let bestScore = -1;
    for (let i = 0; i < this.queue.length; i += 1) {
      const item = this.queue[i];
      if (item.sourceKind === 'full' && this.activeFullLoads >= maxFullActive) continue;
      const score = scoreQueueItem(item);
      if (score > bestScore) {
        bestScore = score;
        bestIndex = i;
      }
    }
    return bestIndex;
  }

  private async loadThumb(item: ThumbnailQueueItem): Promise<void> {
    const entry = this.cache.get(item.hash);
    if (!entry) return;
    if (entry.thumb && !needsUpgrade(entry, item)) return;

    const controller = new AbortController();
    this.inFlight.set(item.hash, {
      controller,
      y: item.y,
      sourceKind: item.sourceKind,
      priority: item.priority,
      requestedLongEdge: item.requestedLongEdge,
    });
    this.activeLoads += 1;
    if (item.sourceKind === 'full') this.activeFullLoads += 1;
    this.markLoading(entry);
    try {
      const response = await fetch(item.url, { signal: controller.signal });
      if (!response.ok) throw new Error(`thumbnail fetch failed: ${response.status}`);
      const blob = await response.blob();
      const bitmap = await createBitmap(blob, item);
      this.applyBitmap(item.hash, bitmap, item.sourceKind, item.requestedLongEdge);
    } catch {
      if (controller.signal.aborted) return;
      const current = this.cache.get(item.hash);
      if (current && item.sourceKind === 'thumbnail' && !current.retryQueued) {
        this.queueRepairRetry(current, item);
      } else if (current) {
        this.markError(current);
        this.onDirty();
      }
    } finally {
      this.inFlight.delete(item.hash);
      this.activeLoads = Math.max(0, this.activeLoads - 1);
      if (item.sourceKind === 'full') {
        this.activeFullLoads = Math.max(0, this.activeFullLoads - 1);
      }
      this.pump();
    }
  }

  private applyBitmap(hash: string, bitmap: ImageBitmap, sourceKind: ThumbnailSourceKind, loadedLongEdge: number): void {
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
    entry.sourceKind = sourceKind;
    entry.loadedLongEdge = loadedLongEdge;
    this.loadedHashes.add(hash);
    this.onDirty();
  }

  private pruneCache(): void {
    if (this.cache.size <= THUMBNAIL_PIPELINE_MAX_ENTRIES) return;
    const target = THUMBNAIL_PIPELINE_MAX_ENTRIES - 100;
    const entries = Array.from(this.cache.entries());
    entries.sort((a, b) => a[1].lastAccessed - b[1].lastAccessed);
    for (const [hash, entry] of entries) {
      if (this.cache.size <= target) break;
      if (entry.state === 'queued' || entry.state === 'loading') continue;
      entry.thumb?.close();
      this.cache.delete(hash);
      this.loadedHashes.delete(hash);
      this.inFlight.get(hash)?.controller.abort();
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
      sourceKind: 'thumbnail',
      loadedLongEdge: 0,
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
        this.ensure(item.hash, { y: item.y });
      });
  }
}

function buildRequest(hash: string, args: EnsureThumbnailArgs): ThumbnailQueueItem | null {
  const y = args.y ?? 0;
  const requestedDisplayLongEdge = Math.max(1, Math.round(Math.max(args.drawWidth ?? 0, args.drawHeight ?? 0)));
  const canUseFullQuality = isEligibleForFullQuality(args);

  if (
    !canUseFullQuality
    || requestedDisplayLongEdge <= Math.round(THUMBNAIL_PIPELINE_SOURCE_EDGE * THUMBNAIL_PIPELINE_FULL_QUALITY_THRESHOLD)
  ) {
      return {
        hash,
        url: mediaThumbnailUrl(hash),
        y,
        sourceKind: 'thumbnail',
        priority: getRequestPriority(args),
        requestedLongEdge: THUMBNAIL_PIPELINE_SOURCE_EDGE,
      };
  }

  const resize = computeResize(
    args.sourceWidth ?? null,
    args.sourceHeight ?? null,
    quantizeLongEdge(requestedDisplayLongEdge),
  );
  return {
    hash,
    url: mediaFileUrl(hash, args.mime!),
    y,
    sourceKind: 'full',
    priority: getRequestPriority(args),
    requestedLongEdge: resize.longEdge,
    resizeWidth: resize.width,
    resizeHeight: resize.height,
  };
}

function isEligibleForFullQuality(args: EnsureThumbnailArgs): boolean {
  return Boolean(
    args.mime?.startsWith('image/')
    && args.sourceWidth
    && args.sourceHeight
    && args.drawWidth
    && args.drawHeight,
  );
}

function quantizeLongEdge(value: number): number {
  const step = 128;
  return Math.min(
    THUMBNAIL_PIPELINE_MAX_FULL_LONG_EDGE,
    Math.max(THUMBNAIL_PIPELINE_SOURCE_EDGE, Math.ceil(value / step) * step),
  );
}

function getRequestPriority(args: EnsureThumbnailArgs): ThumbnailRequestPriority {
  return args.drawWidth && args.drawHeight ? 'visible' : 'prefetch';
}

function computeResize(sourceWidth: number | null, sourceHeight: number | null, longEdge: number) {
  const width = Math.max(1, sourceWidth ?? longEdge);
  const height = Math.max(1, sourceHeight ?? longEdge);
  if (width >= height) {
    return {
      width: longEdge,
      height: Math.max(1, Math.round((longEdge * height) / width)),
      longEdge,
    };
  }
  return {
    width: Math.max(1, Math.round((longEdge * width) / height)),
    height: longEdge,
    longEdge,
  };
}

async function createBitmap(blob: Blob, item: ThumbnailQueueItem): Promise<ImageBitmap> {
  if (item.sourceKind === 'full' && item.resizeWidth && item.resizeHeight) {
    return createImageBitmap(blob, {
      resizeWidth: item.resizeWidth,
      resizeHeight: item.resizeHeight,
      resizeQuality: 'high',
    });
  }
  return createImageBitmap(blob);
}

function needsUpgrade(entry: ThumbnailPipelineEntry, request: { sourceKind: ThumbnailSourceKind; requestedLongEdge: number }): boolean {
  return needsUpgradeState(entry.sourceKind, entry.loadedLongEdge, request.sourceKind, request.requestedLongEdge);
}

function needsUpgradeState(
  currentSourceKind: ThumbnailSourceKind,
  currentLongEdge: number,
  requestedSourceKind: ThumbnailSourceKind,
  requestedLongEdge: number,
): boolean {
  if (requestedSourceKind === 'full' && currentSourceKind !== 'full') return true;
  if (requestedSourceKind === currentSourceKind && requestedLongEdge > currentLongEdge * 1.15) return true;
  return false;
}

function scoreQueueItem(item: ThumbnailQueueItem): number {
  if (item.priority === 'visible' && item.sourceKind === 'full') return 4;
  if (item.priority === 'visible' && item.sourceKind === 'thumbnail') return 3;
  if (item.priority === 'prefetch' && item.sourceKind === 'thumbnail') return 2;
  return 1;
}

export const __private__ = {
  buildRequest,
  needsUpgradeState,
  quantizeLongEdge,
  scoreQueueItem,
};
