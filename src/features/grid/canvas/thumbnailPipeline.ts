function mediaThumbnailUrl(hash: string): string {
  return `media://localhost/thumb/${hash}.jpg`;
}
import {
  decodeThumbnailInWorker,
  getThumbnailDecodeWorkerStats,
  setThumbnailDecodeLateResponseListener,
} from './thumbnailDecodeClient';
// Pipeline policy constants
export const THUMBNAIL_PIPELINE_MAX_ENTRIES = 200;
export const THUMBNAIL_PIPELINE_MAX_CONCURRENT_VISIBLE = 12;
export const THUMBNAIL_PIPELINE_MAX_CONCURRENT_PREFETCH = 4;
export const THUMBNAIL_PIPELINE_SOURCE_EDGE = 750;
/** Reveal duration used by CanvasGrid's stagger system. */
export const REVEAL_DURATION_MS = 150;
import type {
  ThumbnailInFlightItem,
  ThumbnailPipelineEntry,
  ThumbnailPipelineStats,
  ThumbnailQueueItem,
  ThumbnailRequestPriority,
  ThumbnailPipelineTraceEvent,
  EnsureThumbnailArgs,
} from './thumbnailPipelineTypes';
import {
  type CanvasScrollState,
  createIdleCanvasScrollState,
} from './scrollState';

export type { ThumbnailPipelineEntry, ThumbnailPipelineStats, EnsureThumbnailArgs } from './thumbnailPipelineTypes';

type TraceListener = (event: ThumbnailPipelineTraceEvent) => void;

export class ThumbnailPipeline {
  private cache = new Map<string, ThumbnailPipelineEntry>();
  private queueMap = new Map<string, ThumbnailQueueItem>();
  private inFlight = new Map<string, ThumbnailInFlightItem>();
  private activeLoads = 0;
  private activeVisibleLoads = 0;
  private activePrefetchLoads = 0;
  private accessCounter = 0;
  private scrollState: CanvasScrollState = createIdleCanvasScrollState();
  private destroyed = false;
  private totalBytes = 0;
  private onDirty: () => void;
  private onTraceEvent: TraceListener | null = null;
  private generationByHash = new Map<string, number>();

  constructor(onDirty: () => void = () => {}) {
    this.onDirty = onDirty;
    setThumbnailDecodeLateResponseListener((meta) => {
      this.emitTrace('late_worker_response', meta ?? {});
    });
  }

  setOnDirty(onDirty: () => void): void {
    this.onDirty = onDirty;
  }

  setTraceListener(listener: TraceListener | null): void {
    this.onTraceEvent = listener;
  }

  setScrollState(nextState: CanvasScrollState): void {
    this.scrollState = nextState;
    this.pump();
  }

  get(hash: string): ThumbnailPipelineEntry | null {
    const entry = this.cache.get(hash);
    if (!entry) return null;
    this.touch(entry);
    return entry;
  }

  promote(hash: string): boolean {
    const entry = this.cache.get(hash);
    if (!entry?.thumb) return false;
    if (entry.state === 'shown') return false;
    entry.state = 'shown';
    this.emitTrace('bitmap_promoted', {
      hash,
    });
    return true;
  }

  ensure(hash: string, args: EnsureThumbnailArgs = {}): void {
    if (this.destroyed || !hash) return;

    const entry = this.getOrCreateEntry(hash);
    const request = buildRequest(hash, args);
    if (entry.thumb && entry.state === 'shown' && entry.bytes > 0) {
      this.emitTrace('cache_hit', {
        hash,
        priority: request.priority,
      });
      return;
    }

    if (entry.state === 'queued' || entry.state === 'loading') {
      const queued = this.queueMap.get(hash);
      if (queued && queued.priority === request.priority) return;
      const inFlight = this.inFlight.get(hash);
      if (inFlight && inFlight.priority === request.priority) return;
    }

    const generation = (this.generationByHash.get(hash) ?? 0) + 1;
    this.generationByHash.set(hash, generation);
    this.markQueued(entry);
    request.generation = generation;
    this.queueMap.set(hash, request);
    this.emitTrace('queue_enqueued', {
      hash,
      priority: request.priority,
      y: request.y,
      requestedLongEdge: request.requestedLongEdge,
      generation,
    });
    this.pump();
  }

  /** Reset reveal state for entries outside the visible window so they re-fade on scroll back. */
  resetRevealOutsideWindow(visibleHashes: Set<string>): void {
    for (const [hash, entry] of this.cache) {
      if (visibleHashes.has(hash)) continue;
      if (entry.animateIn) {
        entry.animateIn = false;
        entry.revealStartedAt = 0;
      }
    }
  }

  cancelOutsideWindow(top: number, bottom: number): void {
    for (const [hash, item] of this.queueMap) {
      if (item.y >= top && item.y <= bottom) continue;
      this.queueMap.delete(hash);
      const entry = this.cache.get(hash);
      if (entry && !entry.thumb) this.resetEntry(entry);
      this.emitTrace('queue_became_stale', {
        hash,
        priority: item.priority,
        reason: 'cancel_window',
      });
    }

    for (const [hash, inFlight] of this.inFlight) {
      if (inFlight.y >= top && inFlight.y <= bottom) continue;
      inFlight.cancel();
      const entry = this.cache.get(hash);
      if (entry && entry.state === 'loading') this.resetEntry(entry);
      this.emitTrace('inflight_canceled', {
        hash,
        priority: inFlight.priority,
        reason: 'cancel_window',
      });
    }
  }

  getEvictCandidatesBatch(
    keepHashes: Set<string>,
    limit: number,
    cursor: number,
  ): { evicted: string[]; nextCursor: number } {
    const evicted: string[] = [];
    const keys = Array.from(this.cache.keys());
    if (keys.length === 0) return { evicted, nextCursor: 0 };
    let idx = cursor % keys.length;
    for (let checked = 0; checked < limit && checked < keys.length; checked += 1) {
      const hash = keys[idx];
      const entry = this.cache.get(hash);
      if (entry?.thumb && !keepHashes.has(hash) && entry.state !== 'queued' && entry.state !== 'loading') {
        evicted.push(hash);
      }
      idx = (idx + 1) % keys.length;
    }
    return { evicted, nextCursor: idx };
  }

  evictHashes(hashes: string[]): void {
    for (const hash of hashes) {
      const entry = this.cache.get(hash);
      if (!entry || !entry.thumb) continue;
      this.totalBytes -= entry.bytes;
      entry.thumb.close();
      entry.thumb = null;
      entry.bytes = 0;
      entry.animateIn = false;
      entry.revealStartedAt = 0;
      this.resetEntry(entry);
      this.emitTrace('evicted', {
        hash,
      });
    }
  }

  getStats(): ThumbnailPipelineStats {
    const workerStats = getThumbnailDecodeWorkerStats();
    return {
      queueDepth: this.queueMap.size,
      activeLoads: this.activeLoads,
      cacheEntries: this.cache.size,
      totalBytes: this.totalBytes,
      scrollPhase: this.scrollState.phase,
      scrollDirection: this.scrollState.direction,
      scrollVelocityPxPerSec: this.scrollState.velocityPxPerSec,
      droppedLateWorkerResults: workerStats.droppedLateResponses,
    };
  }

  clear(): void {
    this.destroyed = true;
    for (const inFlight of this.inFlight.values()) inFlight.cancel();
    this.inFlight.clear();
    this.queueMap.clear();
    for (const entry of this.cache.values()) {
      entry.thumb?.close();
    }
    this.cache.clear();
    this.activeLoads = 0;
    this.activeVisibleLoads = 0;
    this.activePrefetchLoads = 0;
    this.totalBytes = 0;
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
    let best: ThumbnailQueueItem | null = null;
    let bestScore = -1;
    for (const item of this.queueMap.values()) {
      if (!canStartLoad(item, this.activeVisibleLoads, this.activePrefetchLoads)) {
        continue;
      }
      const score = scoreQueueItem(item);
      if (score > bestScore) {
        best = item;
        bestScore = score;
      }
    }
    return best;
  }

  private startLoad(item: ThumbnailQueueItem): void {
    const entry = this.cache.get(item.hash);
    if (!entry) return;

    let finished = false;
    const finish = () => {
      if (finished) return;
      finished = true;
      this.inFlight.delete(item.hash);
      this.activeLoads = Math.max(0, this.activeLoads - 1);
      if (item.priority === 'visible') this.activeVisibleLoads = Math.max(0, this.activeVisibleLoads - 1);
      else this.activePrefetchLoads = Math.max(0, this.activePrefetchLoads - 1);
      this.pump();
    };

    const controller = new AbortController();
    this.inFlight.set(item.hash, {
      cancel: () => {
        controller.abort();
        finish();
      },
      y: item.y,
      priority: item.priority,
      requestedLongEdge: item.requestedLongEdge,
      queuedAt: item.queuedAt,
      generation: item.generation,
    });

    this.activeLoads += 1;
    if (item.priority === 'visible') this.activeVisibleLoads += 1;
    else this.activePrefetchLoads += 1;
    this.markLoading(entry);
    this.emitTrace('load_started', {
      hash: item.hash,
      priority: item.priority,
      generation: item.generation,
    });

    void loadBitmap(item, controller.signal)
      .then(({ bitmap }) => {
        if (controller.signal.aborted) {
          bitmap.close();
          return;
        }
        this.applyBitmap(item.hash, bitmap);
      })
      .catch(() => {
        if (controller.signal.aborted) return;
        const current = this.cache.get(item.hash);
        if (current) {
          current.state = 'error';
          this.onDirty();
        }
      })
      .finally(() => {
        finish();
      });
  }

  private applyBitmap(hash: string, bitmap: ImageBitmap): void {
    const entry = this.cache.get(hash);
    if (!entry) {
      bitmap.close();
      this.emitTrace('stale_result_dropped', {
        hash,
        reason: 'missing_entry',
      });
      return;
    }

    const expectedGeneration = this.generationByHash.get(hash) ?? 0;
    const inFlight = this.inFlight.get(hash);
    if (inFlight && inFlight.generation !== expectedGeneration) {
      bitmap.close();
      this.emitTrace('stale_result_dropped', {
        hash,
        reason: 'generation_mismatch',
        expectedGeneration,
        actualGeneration: inFlight.generation,
      });
      return;
    }

    const isUpgrade = entry.thumb != null;
    this.totalBytes -= entry.bytes;
    entry.thumb?.close();
    entry.thumb = bitmap;
    entry.bytes = bitmap.width * bitmap.height * 4;
    this.totalBytes += entry.bytes;
    entry.state = 'shown';
    if (isUpgrade) {
      // Thumbnail → full-quality: silent swap, no fade.
      entry.animateIn = false;
    } else {
      // Fresh load or re-load after eviction: fade in.
      entry.animateIn = true;
      entry.revealStartedAt = performance.now();
    }
    this.pruneCache();
    this.emitTrace('bitmap_ready', {
      hash,
      bytes: entry.bytes,
    });
    this.onDirty();
  }

  private pruneCache(): void {
    if (this.cache.size <= THUMBNAIL_PIPELINE_MAX_ENTRIES) return;
    const target = THUMBNAIL_PIPELINE_MAX_ENTRIES - 25;
    const threshold = this.accessCounter - target;
    for (const [hash, entry] of this.cache) {
      if (this.cache.size <= target) break;
      if (entry.state === 'queued' || entry.state === 'loading') continue;
      if (entry.lastAccessed >= threshold) continue;
      if (entry.thumb) {
        this.totalBytes -= entry.bytes;
        entry.thumb.close();
      }
      this.cache.delete(hash);
      this.emitTrace('evicted', {
        hash,
        reason: 'cache_prune',
      });
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
      bytes: 0,
      animateIn: false,
      revealStartedAt: 0,
    };
    this.cache.set(hash, entry);
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

  private resetEntry(entry: ThumbnailPipelineEntry): void {
    entry.state = 'idle';
  }

  private emitTrace(type: string, payload: Record<string, unknown>): void {
    this.onTraceEvent?.({ type, payload });
  }
}

function buildRequest(hash: string, args: EnsureThumbnailArgs): ThumbnailQueueItem {
  const requestedDisplayLongEdge = Math.max(1, Math.round(Math.max(args.drawWidth ?? 0, args.drawHeight ?? 0)));
  return {
    hash,
    url: mediaThumbnailUrl(hash),
    y: args.y ?? 0,
    priority: getRequestPriority(args),
    requestedLongEdge: Math.max(THUMBNAIL_PIPELINE_SOURCE_EDGE, requestedDisplayLongEdge),
    queuedAt: performance.now(),
    generation: 0,
  };
}

function getRequestPriority(args: EnsureThumbnailArgs): ThumbnailRequestPriority {
  return args.drawWidth && args.drawHeight ? 'visible' : 'prefetch';
}

async function loadBitmap(
  item: ThumbnailQueueItem,
  signal: AbortSignal,
): Promise<{ bitmap: ImageBitmap; decodeDurationMs: number }> {
  const startedAt = performance.now();
  const meta = {
    hash: item.hash,
    priority: item.priority,
    generation: item.generation,
  };
  const workerResult = decodeThumbnailInWorker(item.url, signal, meta);
  if (workerResult) {
    try {
      const { bitmap, durationMs } = await workerResult;
      return { bitmap, decodeDurationMs: durationMs };
    } catch (error) {
      if (signal.aborted) throw error;
    }
  }

  try {
    const response = await fetch(item.url, { signal });
    if (!response.ok) throw new Error(`thumbnail fetch failed: ${response.status}`);
    const blob = await response.blob();
    return {
      bitmap: await createImageBitmap(blob),
      decodeDurationMs: performance.now() - startedAt,
    };
  } catch (error) {
    if (signal.aborted) throw error;
    return {
      bitmap: await loadBitmapViaImage(item.url, signal),
      decodeDurationMs: performance.now() - startedAt,
    };
  }
}

function loadBitmapViaImage(url: string, signal: AbortSignal): Promise<ImageBitmap> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    let done = false;

    const cleanup = () => {
      img.onload = null;
      img.onerror = null;
      signal.removeEventListener('abort', onAbort);
    };

    const onAbort = () => {
      if (done) return;
      done = true;
      cleanup();
      reject(new DOMException('Aborted', 'AbortError'));
    };

    img.crossOrigin = 'anonymous';
    img.onload = () => {
      if (done) return;
      done = true;
      cleanup();
      createImageBitmap(img).then(resolve, reject);
    };
    img.onerror = () => {
      if (done) return;
      done = true;
      cleanup();
      reject(new Error('thumbnail image load failed'));
    };

    signal.addEventListener('abort', onAbort, { once: true });
    if (signal.aborted) {
      onAbort();
      return;
    }
    img.src = url;
  });
}

function scoreQueueItem(item: ThumbnailQueueItem): number {
  return item.priority === 'visible' ? 2 : 1;
}

function canStartLoad(item: ThumbnailQueueItem, activeVisibleLoads: number, activePrefetchLoads: number): boolean {
  if (item.priority === 'prefetch') {
    return activePrefetchLoads < THUMBNAIL_PIPELINE_MAX_CONCURRENT_PREFETCH;
  }
  return activeVisibleLoads < THUMBNAIL_PIPELINE_MAX_CONCURRENT_VISIBLE;
}
