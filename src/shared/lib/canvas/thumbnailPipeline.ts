import { api } from '#desktop/api';
import { mediaFileUrl, mediaThumbnailUrl } from '../mediaUrl';
import { decodeThumbnailInWorker, getThumbnailDecodeWorkerStats } from './thumbnailDecodeClient';
import { enqueueMediaQosTask, type MediaQosLane, type MediaQosTaskHandle } from '../mediaQosScheduler';
import {
  THUMBNAIL_PIPELINE_FULL_QUALITY_THRESHOLD,
  THUMBNAIL_PIPELINE_MAX_VISIBLE_ACTIVE_FAST,
  THUMBNAIL_PIPELINE_MAX_VISIBLE_ACTIVE_SLOW,
  THUMBNAIL_PIPELINE_MAX_VISIBLE_ACTIVE_IDLE,
  THUMBNAIL_PIPELINE_MAX_PREFETCH_ACTIVE_FAST,
  THUMBNAIL_PIPELINE_MAX_PREFETCH_ACTIVE_SLOW,
  THUMBNAIL_PIPELINE_MAX_PREFETCH_ACTIVE_IDLE,
  THUMBNAIL_PIPELINE_MAX_FULL_ACTIVE_FAST,
  THUMBNAIL_PIPELINE_MAX_FULL_ACTIVE_SLOW,
  THUMBNAIL_PIPELINE_MAX_FULL_ACTIVE_IDLE,
  THUMBNAIL_PIPELINE_MAX_FULL_LONG_EDGE,
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
import {
  type CanvasScrollPhase,
  type CanvasScrollState,
  createIdleCanvasScrollState,
} from './scrollState';

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
  private queueMap = new Map<string, ThumbnailQueueItem>();
  private activeLoads = 0;
  private activeVisibleThumbLoads = 0;
  private activePrefetchThumbLoads = 0;
  private activeFullLoads = 0;
  private accessCounter = 0;
  private scrollState: CanvasScrollState = createIdleCanvasScrollState();
  private destroyed = false;
  private loadedHashes = new Set<string>();
  private inFlight = new Map<string, ThumbnailInFlightItem>();
  private cacheHitCount = 0;
  private cacheMissCount = 0;
  private visibleThumbWaitTotalMs = 0;
  private visibleThumbWaitSamples = 0;
  private decodeTotalMs = 0;
  private decodeSamples = 0;
  private cancelCountByClass = {
    visibleThumb: 0,
    prefetchThumb: 0,
    visibleFull: 0,
  };

  private onDirty: () => void;

  constructor(onDirty: () => void = () => {}) {
    this.onDirty = onDirty;
  }

  setOnDirty(onDirty: () => void): void {
    this.onDirty = onDirty;
  }

  setScrollState(nextState: CanvasScrollState): void {
    const prevPhase = this.scrollState.phase;
    this.scrollState = nextState;

    if (nextState.phase === 'fast' && prevPhase !== 'fast') {
      for (const [hash, item] of this.queueMap) {
        if (!isNonVisibleWork(item)) continue;
        this.queueMap.delete(hash);
        this.recordCancellation(item.sourceKind, item.priority);
        const entry = this.cache.get(hash);
        if (entry && !entry.thumb) this.resetEntry(entry);
      }

      for (const [hash, inFlight] of this.inFlight) {
        if (!isNonVisibleWork(inFlight)) continue;
        this.recordCancellation(inFlight.sourceKind, inFlight.priority);
        inFlight.cancel();
        const entry = this.cache.get(hash);
        if (entry && entry.state === 'loading') this.resetEntry(entry);
      }
    }

    this.pump();
  }

  setScrolling(active: boolean): void {
    this.setScrollState({
      phase: active ? 'slow' : 'idle',
      direction: this.scrollState.direction,
      velocityPxPerSec: active ? Math.max(this.scrollState.velocityPxPerSec, 1) : 0,
    });
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
      if (!needsUpgrade(entry, request)) {
        this.cacheHitCount += 1;
        return;
      }
    } else if (entry.state === 'queued' || entry.state === 'loading') {
      const active = this.inFlight.get(hash);
      if (active && !needsUpgradeState(active.sourceKind, active.requestedLongEdge, request.sourceKind, request.requestedLongEdge)) {
        return;
      }
      const queued = this.queueMap.get(hash);
      if (queued && !needsUpgradeState(queued.sourceKind, queued.requestedLongEdge, request.sourceKind, request.requestedLongEdge)) {
        return;
      }
    }

    this.cacheMissCount += 1;
    this.markQueued(entry);
    this.queueMap.set(hash, request);
    this.pump();
  }

  cancelOutsideWindow(top: number, bottom: number): void {
    for (const [hash, item] of this.queueMap) {
      if (item.y >= top && item.y <= bottom) continue;
      this.queueMap.delete(hash);
      this.recordCancellation(item.sourceKind, item.priority);
      const entry = this.cache.get(hash);
      if (!entry) continue;
      if (!entry.thumb) this.resetEntry(entry);
    }

    for (const [hash, inFlight] of this.inFlight) {
      const entry = this.cache.get(hash);
      if (!entry || entry.state !== 'loading') continue;
      if (inFlight.y >= top && inFlight.y <= bottom) continue;
      this.recordCancellation(inFlight.sourceKind, inFlight.priority);
      inFlight.cancel();
      this.resetEntry(entry);
    }
  }

  getStats(): ThumbnailPipelineStats {
    const queuedByClass = countQueuedByClass(this.queueMap.values());
    const cacheAccessCount = this.cacheHitCount + this.cacheMissCount;
    const workerStats = getThumbnailDecodeWorkerStats();
    return {
      queueDepth: this.queueMap.size,
      activeLoads: this.activeLoads,
      pendingThumbs: this.queueMap.size,
      cacheSize: this.cache.size,
      diskSpeed: 'normal',
      activeByClass: {
        visibleThumb: this.activeVisibleThumbLoads,
        prefetchThumb: this.activePrefetchThumbLoads,
        visibleFull: this.activeFullLoads,
      },
      queuedByClass,
      cancelCountByClass: { ...this.cancelCountByClass },
      visibleThumbWaitMsAvg: this.visibleThumbWaitSamples > 0
        ? this.visibleThumbWaitTotalMs / this.visibleThumbWaitSamples
        : 0,
      decodeMsAvg: this.decodeSamples > 0 ? this.decodeTotalMs / this.decodeSamples : 0,
      cacheHitRate: cacheAccessCount > 0 ? this.cacheHitCount / cacheAccessCount : 0,
      droppedLateWorkerResults: workerStats.droppedLateResponses,
      scrollPhase: this.scrollState.phase,
      scrollDirection: this.scrollState.direction,
      scrollVelocityPxPerSec: this.scrollState.velocityPxPerSec,
    };
  }

  destroy(): void {
    this.destroyed = true;
    for (const inFlight of this.inFlight.values()) inFlight.cancel();
    this.inFlight.clear();
    this.queueMap.clear();
    for (const entry of this.cache.values()) {
      entry.thumb?.close();
    }
    this.cache.clear();
  }

  private pump(): void {
    if (this.destroyed) return;
    while (true) {
      const next = this.selectNextQueueItem();
      if (!next) break;
      this.queueMap.delete(next.hash);
      this.startLoad(next);
    }
  }

  private selectNextQueueItem(): ThumbnailQueueItem | null {
    const budgets = getActiveBudgets(this.scrollState.phase);
    let best: ThumbnailQueueItem | null = null;
    let bestScore = -1;
    for (const item of this.queueMap.values()) {
      if (!canStartQueueItem({
        item,
        budgets,
        scrollPhase: this.scrollState.phase,
        activeVisibleThumbLoads: this.activeVisibleThumbLoads,
        activePrefetchThumbLoads: this.activePrefetchThumbLoads,
        activeFullLoads: this.activeFullLoads,
      })) continue;
      const score = scoreQueueItem(item);
      if (score > bestScore) {
        bestScore = score;
        best = item;
      }
    }
    return best;
  }

  private startLoad(item: ThumbnailQueueItem): void {
    const entry = this.cache.get(item.hash);
    if (!entry) return;
    if (entry.thumb && !needsUpgrade(entry, item)) return;

    let finished = false;
    let handle: MediaQosTaskHandle | null = null;
    const finish = () => {
      if (finished) return;
      finished = true;
      this.inFlight.delete(item.hash);
      this.activeLoads = Math.max(0, this.activeLoads - 1);
      if (item.sourceKind === 'full') {
        this.activeFullLoads = Math.max(0, this.activeFullLoads - 1);
      } else if (item.priority === 'visible') {
        this.activeVisibleThumbLoads = Math.max(0, this.activeVisibleThumbLoads - 1);
      } else {
        this.activePrefetchThumbLoads = Math.max(0, this.activePrefetchThumbLoads - 1);
      }
      this.pump();
    };

    this.inFlight.set(item.hash, {
      cancel: () => {
        handle?.cancel();
        finish();
      },
      y: item.y,
      sourceKind: item.sourceKind,
      priority: item.priority,
      requestedLongEdge: item.requestedLongEdge,
      queuedAt: item.queuedAt,
    });
    this.activeLoads += 1;
    if (item.sourceKind === 'full') {
      this.activeFullLoads += 1;
    } else if (item.priority === 'visible') {
      this.activeVisibleThumbLoads += 1;
    } else {
      this.activePrefetchThumbLoads += 1;
    }
    this.markLoading(entry);
    handle = enqueueMediaQosTask({
      lane: getQosLane(item),
      priority: getQosPriority(item),
      heavy: item.sourceKind === 'full',
      run: async (signal) => {
        try {
          const { bitmap, decodeDurationMs } = await loadBitmap(item, signal);
          if (signal.aborted) {
            bitmap?.close();
            return;
          }
          this.decodeTotalMs += decodeDurationMs;
          this.decodeSamples += 1;
          this.applyBitmap(item.hash, bitmap, item.sourceKind, item.requestedLongEdge, item);
        } catch {
          if (signal.aborted) return;
          const current = this.cache.get(item.hash);
          if (current && item.sourceKind === 'thumbnail' && !current.retryQueued) {
            this.queueRepairRetry(current, item);
          } else if (current) {
            this.markError(current);
            this.onDirty();
          }
        } finally {
          finish();
        }
      },
    });
  }

  private applyBitmap(
    hash: string,
    bitmap: ImageBitmap,
    sourceKind: ThumbnailSourceKind,
    loadedLongEdge: number,
    item: ThumbnailQueueItem,
  ): void {
    const entry = this.cache.get(hash);
    if (!entry) {
      bitmap.close();
      return;
    }
    if (item.priority === 'visible' && item.sourceKind === 'thumbnail') {
      this.visibleThumbWaitTotalMs += Math.max(0, performance.now() - item.queuedAt);
      this.visibleThumbWaitSamples += 1;
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

  private static readonly MAX_LOADED_HASHES = 10_000;

  private pruneCache(): void {
    // Cap loadedHashes independently — this set only controls animation.
    // A larger budget (5× cache size) prevents re-fading recently-viewed
    // tiles when the user scrolls back (reference application-style `.from-cache` behavior).
    if (this.loadedHashes.size > ThumbnailPipeline.MAX_LOADED_HASHES) {
      let toDelete = this.loadedHashes.size - ThumbnailPipeline.MAX_LOADED_HASHES + 500;
      for (const h of this.loadedHashes) {
        if (toDelete-- <= 0) break;
        this.loadedHashes.delete(h);
      }
    }

    if (this.cache.size <= THUMBNAIL_PIPELINE_MAX_ENTRIES) return;
    const target = THUMBNAIL_PIPELINE_MAX_ENTRIES - 100;
    // O(N) eviction: entries with lastAccessed below the threshold are older
    // than we want to keep. accessCounter is monotonically increasing, so
    // entries not touched in the last ~target accesses are eviction candidates.
    const threshold = this.accessCounter - target;
    for (const [hash, entry] of this.cache) {
      if (this.cache.size <= target) break;
      if (entry.state === 'queued' || entry.state === 'loading') continue;
      if (entry.lastAccessed >= threshold) continue;
      entry.thumb?.close();
      this.cache.delete(hash);
      // NOTE: do NOT delete from loadedHashes — keeps reveal memory intact
      this.inFlight.get(hash)?.cancel();
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

  private recordCancellation(sourceKind: ThumbnailSourceKind, priority: ThumbnailRequestPriority): void {
    if (sourceKind === 'full') {
      this.cancelCountByClass.visibleFull += 1;
      return;
    }
    if (priority === 'prefetch') {
      this.cancelCountByClass.prefetchThumb += 1;
      return;
    }
    this.cancelCountByClass.visibleThumb += 1;
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
  const queuedAt = performance.now();

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
        queuedAt,
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
    queuedAt,
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
      resizeQuality: 'medium',
    });
  }
  return createImageBitmap(blob);
}

async function loadBitmap(
  item: ThumbnailQueueItem,
  signal: AbortSignal,
): Promise<{ bitmap: ImageBitmap; decodeDurationMs: number }> {
  const mainThreadStartedAt = performance.now();
  const workerResult = decodeThumbnailInWorker(item, signal);
  if (workerResult) {
    try {
      const { bitmap, durationMs } = await workerResult;
      return { bitmap, decodeDurationMs: durationMs };
    } catch (error) {
      if (signal.aborted) throw error;
    }
  }

  const response = await fetch(item.url, { signal });
  if (!response.ok) throw new Error(`thumbnail fetch failed: ${response.status}`);
  const blob = await response.blob();
  return {
    bitmap: await createBitmap(blob, item),
    decodeDurationMs: performance.now() - mainThreadStartedAt,
  };
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
  if (item.priority === 'visible' && item.sourceKind === 'thumbnail') return 3;
  if (item.priority === 'visible' && item.sourceKind === 'full') return 2;
  if (item.priority === 'prefetch' && item.sourceKind === 'thumbnail') return 1;
  return 1;
}

function getQosLane(item: ThumbnailQueueItem): MediaQosLane {
  if (item.sourceKind === 'full') return 'grid_visible_full';
  return item.priority === 'visible' ? 'grid_visible_thumb' : 'grid_prefetch_thumb';
}

function getQosPriority(item: ThumbnailQueueItem): number {
  if (item.sourceKind === 'full') return 20;
  return item.priority === 'visible' ? 0 : 50;
}

function getActiveBudgets(scrollPhase: CanvasScrollPhase) {
  return {
    maxVisibleThumbs: scrollPhase === 'fast'
      ? THUMBNAIL_PIPELINE_MAX_VISIBLE_ACTIVE_FAST
      : scrollPhase === 'slow'
        ? THUMBNAIL_PIPELINE_MAX_VISIBLE_ACTIVE_SLOW
        : THUMBNAIL_PIPELINE_MAX_VISIBLE_ACTIVE_IDLE,
    maxPrefetchThumbs: scrollPhase === 'fast'
      ? THUMBNAIL_PIPELINE_MAX_PREFETCH_ACTIVE_FAST
      : scrollPhase === 'slow'
        ? THUMBNAIL_PIPELINE_MAX_PREFETCH_ACTIVE_SLOW
        : THUMBNAIL_PIPELINE_MAX_PREFETCH_ACTIVE_IDLE,
    maxFull: scrollPhase === 'fast'
      ? THUMBNAIL_PIPELINE_MAX_FULL_ACTIVE_FAST
      : scrollPhase === 'slow'
        ? THUMBNAIL_PIPELINE_MAX_FULL_ACTIVE_SLOW
        : THUMBNAIL_PIPELINE_MAX_FULL_ACTIVE_IDLE,
  };
}

function canStartQueueItem(args: {
  item: ThumbnailQueueItem;
  budgets: ReturnType<typeof getActiveBudgets>;
  scrollPhase: CanvasScrollPhase;
  activeVisibleThumbLoads: number;
  activePrefetchThumbLoads: number;
  activeFullLoads: number;
}): boolean {
  const {
    item,
    budgets,
    scrollPhase,
    activeVisibleThumbLoads,
    activePrefetchThumbLoads,
    activeFullLoads,
  } = args;

  if (item.sourceKind === 'full') return activeFullLoads < budgets.maxFull;
  if (item.priority === 'prefetch') {
    if (scrollPhase === 'fast') return false;
    return activePrefetchThumbLoads < budgets.maxPrefetchThumbs;
  }
  return activeVisibleThumbLoads < budgets.maxVisibleThumbs;
}

function isNonVisibleWork(item: Pick<ThumbnailQueueItem, 'sourceKind' | 'priority'> | Pick<ThumbnailInFlightItem, 'sourceKind' | 'priority'>): boolean {
  return item.sourceKind === 'full' || item.priority === 'prefetch';
}

function countQueuedByClass(items: Iterable<ThumbnailQueueItem>) {
  const queuedByClass = {
    visibleThumb: 0,
    prefetchThumb: 0,
    visibleFull: 0,
  };
  for (const item of items) {
    if (item.sourceKind === 'full') queuedByClass.visibleFull += 1;
    else if (item.priority === 'prefetch') queuedByClass.prefetchThumb += 1;
    else queuedByClass.visibleThumb += 1;
  }
  return queuedByClass;
}

export const __private__ = {
  buildRequest,
  needsUpgradeState,
  quantizeLongEdge,
  scoreQueueItem,
  getActiveBudgets,
  canStartQueueItem,
};
