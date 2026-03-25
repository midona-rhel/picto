/**
 * Image pipeline — loads, caches, and evicts images for the canvas grid.
 *
 * The tile doesn't know whether it's getting a thumbnail or full-quality image.
 * It requests an image at a display size; the pipeline decides what to serve.
 * Currently all requests load thumbnails. Full-quality upgrade can be added
 * without changing the renderer contract.
 *
 * Memory-budgeted: tracks estimated VRAM per bitmap, evicts LRU when over budget.
 */

export type OnImageLoaded = () => void;

type QueuePriority = 0 | 1 | 2;

export interface ImageRequest {
  hash: string;
  displayWidth: number;
  displayHeight: number;
}

export interface RequestGroups {
  visible: Array<string | ImageRequest>;
  ahead: Array<string | ImageRequest>;
  behind: Array<string | ImageRequest>;
}

export interface PipelineStats {
  queueDepth: number;
  activeLoads: number;
  cacheEntries: number;
  totalBytes: number;
}

export type PipelineEntryState = 'ready' | 'loading' | 'queued' | 'missing';

interface CacheEntry {
  bitmap: ImageBitmap;
  bytes: number;
  lastUsedAt: number;
}

interface QueueEntry {
  hash: string;
  priority: QueuePriority;
  seq: number;
}

const MAX_CONCURRENT_LOADS = 6;
const VRAM_BUDGET_BYTES = 1024 * 1024 * 1024; // 1 GB

export class ThumbnailPipeline {
  private cache = new Map<string, CacheEntry>();
  private loading = new Set<string>();
  private queued = new Map<string, QueueEntry>();
  private sequence = 0;
  private totalBytes = 0;
  private onLoaded: OnImageLoaded;
  private notifyScheduled = false;

  constructor(onLoaded: OnImageLoaded) {
    this.onLoaded = onLoaded;
  }

  get(hash: string): ImageBitmap | undefined {
    const entry = this.cache.get(hash);
    if (entry) {
      entry.lastUsedAt = performance.now();
      return entry.bitmap;
    }
    return undefined;
  }

  getState(hash: string): PipelineEntryState {
    if (this.cache.has(hash)) return 'ready';
    if (this.loading.has(hash)) return 'loading';
    if (this.queued.has(hash)) return 'queued';
    return 'missing';
  }

  /** Get all bitmaps for the draw layer. Touches lastUsedAt for visible entries via get(). */
  getAll(): Map<string, CacheEntry> {
    return this.cache;
  }

  request(groups: RequestGroups) {
    this.enqueueGroup(groups.visible, 0);
    this.enqueueGroup(groups.ahead, 1);
    this.enqueueGroup(groups.behind, 2);
    this.drain();
  }

  /** Evict queued and cached items not in the keep set. */
  evictExcept(keepOrA: Set<string> | string[], b?: string[], c?: string[], maxEvictPerTick = 8) {
    let keep: Set<string>;
    if (keepOrA instanceof Set) {
      keep = keepOrA;
    } else {
      keep = new Set<string>();
      for (const h of keepOrA) keep.add(h);
      if (b) for (const h of b) keep.add(h);
      if (c) for (const h of c) keep.add(h);
    }

    for (const hash of this.queued.keys()) {
      if (!keep.has(hash)) this.queued.delete(hash);
    }

    let evicted = 0;
    for (const [hash, entry] of this.cache) {
      if (evicted >= maxEvictPerTick) break;
      if (!keep.has(hash)) {
        this.totalBytes -= entry.bytes;
        entry.bitmap.close();
        this.cache.delete(hash);
        evicted++;
      }
    }
  }

  clear() {
    this.loading.clear();
    this.queued.clear();
    for (const entry of this.cache.values()) {
      entry.bitmap.close();
    }
    this.cache.clear();
    this.totalBytes = 0;
  }

  getStats(): PipelineStats {
    return {
      queueDepth: this.queued.size,
      activeLoads: this.loading.size,
      cacheEntries: this.cache.size,
      totalBytes: this.totalBytes,
    };
  }

  private enqueueGroup(items: Array<string | ImageRequest>, priority: QueuePriority) {
    for (const item of items) {
      const hash = typeof item === 'string' ? item : item.hash;
      if (!hash || this.cache.has(hash) || this.loading.has(hash)) continue;
      const existing = this.queued.get(hash);
      if (existing) {
        if (priority < existing.priority) existing.priority = priority;
        continue;
      }
      this.queued.set(hash, { hash, priority, seq: this.sequence++ });
    }
  }

  private drain() {
    while (this.loading.size < MAX_CONCURRENT_LOADS) {
      const next = this.dequeueNext();
      if (!next) break;
      this.loading.add(next.hash);
      this.loadOne(next.hash);
    }
  }

  private dequeueNext(): QueueEntry | null {
    let best: QueueEntry | null = null;
    for (const entry of this.queued.values()) {
      if (
        !best
        || entry.priority < best.priority
        || (entry.priority === best.priority && entry.seq < best.seq)
      ) {
        best = entry;
      }
    }
    if (!best) return null;
    this.queued.delete(best.hash);
    return best;
  }

  /** Coalesce multiple load completions into a single notification. */
  private scheduleNotify() {
    if (this.notifyScheduled) return;
    this.notifyScheduled = true;
    queueMicrotask(() => {
      this.notifyScheduled = false;
      this.onLoaded();
    });
  }

  /** Evict least-recently-used entries until under VRAM budget. */
  private enforceVramBudget() {
    if (this.totalBytes <= VRAM_BUDGET_BYTES) return;

    // Collect entries sorted by lastUsedAt ascending (oldest first)
    const entries = [...this.cache.entries()].sort(
      (a, b) => a[1].lastUsedAt - b[1].lastUsedAt,
    );

    for (const [hash, entry] of entries) {
      if (this.totalBytes <= VRAM_BUDGET_BYTES) break;
      this.totalBytes -= entry.bytes;
      entry.bitmap.close();
      this.cache.delete(hash);
    }
  }

  private loadOne(hash: string) {
    // Currently always loads thumbnails. A future upgrade could check
    // displayWidth/displayHeight against thumbnail dimensions and load
    // media://host/file/<hash>.<ext> for larger tiles instead.
    const url = `media://host/thumb/${hash}.jpg`;
    const img = new Image();
    img.crossOrigin = 'anonymous';

    img.onload = async () => {
      try {
        if (!this.loading.has(hash)) return;

        const bitmap = await createImageBitmap(img);
        if (!this.loading.has(hash)) {
          bitmap.close();
          return;
        }

        const bytes = bitmap.width * bitmap.height * 4;
        this.loading.delete(hash);
        this.cache.set(hash, { bitmap, bytes, lastUsedAt: performance.now() });
        this.totalBytes += bytes;
        this.enforceVramBudget();
        this.scheduleNotify();
      } catch {
        this.loading.delete(hash);
      } finally {
        this.drain();
      }
    };

    img.onerror = () => {
      this.loading.delete(hash);
      this.drain();
    };

    img.src = url;
  }
}
