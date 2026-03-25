/**
 * Thumbnail pipeline — loads, caches, prioritizes, and evicts thumbnails for the canvas grid.
 *
 * Uses <img> elements to load thumbnails (allowed by img-src CSP), then
 * converts to ImageBitmap for efficient canvas drawing.
 */

export type OnThumbnailLoaded = (hash: string) => void;

type QueuePriority = 0 | 1 | 2;

interface RequestGroups {
  visible: string[];
  ahead: string[];
  behind: string[];
}

interface QueueEntry {
  hash: string;
  priority: QueuePriority;
  seq: number;
}

const MAX_CONCURRENT_LOADS = 6;

export class ThumbnailPipeline {
  private bitmaps = new Map<string, ImageBitmap>();
  private loading = new Set<string>();
  private queued = new Map<string, QueueEntry>();
  private sequence = 0;
  private onLoaded: OnThumbnailLoaded;
  private notifyScheduled = false;

  constructor(onLoaded: OnThumbnailLoaded) {
    this.onLoaded = onLoaded;
  }

  get(hash: string): ImageBitmap | undefined {
    return this.bitmaps.get(hash);
  }

  getAll(): Map<string, ImageBitmap> {
    return this.bitmaps;
  }

  request(groups: RequestGroups) {
    this.enqueue(groups.visible, 0);
    this.enqueue(groups.ahead, 1);
    this.enqueue(groups.behind, 2);
    this.drain();
  }

  /** Evict items not in any of the provided arrays. Avoids allocating a merged Set. */
  evictExcept(a: string[], b: string[], c: string[], maxEvictPerTick = 8) {
    // Build a single lookup set from the three arrays
    const keep = new Set<string>();
    for (const h of a) keep.add(h);
    for (const h of b) keep.add(h);
    for (const h of c) keep.add(h);

    for (const hash of this.queued.keys()) {
      if (!keep.has(hash)) this.queued.delete(hash);
    }

    let evicted = 0;
    for (const [hash, bitmap] of this.bitmaps) {
      if (evicted >= maxEvictPerTick) break;
      if (!keep.has(hash)) {
        bitmap.close();
        this.bitmaps.delete(hash);
        evicted++;
      }
    }
  }

  clear() {
    this.loading.clear();
    this.queued.clear();
    for (const bitmap of this.bitmaps.values()) {
      bitmap.close();
    }
    this.bitmaps.clear();
  }

  /** Coalesce multiple load completions into a single onLoaded notification. */
  private scheduleNotify() {
    if (this.notifyScheduled) return;
    this.notifyScheduled = true;
    queueMicrotask(() => {
      this.notifyScheduled = false;
      this.onLoaded('');
    });
  }

  private enqueue(hashes: string[], priority: QueuePriority) {
    for (const hash of hashes) {
      if (!hash || this.bitmaps.has(hash) || this.loading.has(hash)) continue;
      const existing = this.queued.get(hash);
      if (existing) {
        if (priority < existing.priority) {
          existing.priority = priority;
        }
        continue;
      }
      this.queued.set(hash, {
        hash,
        priority,
        seq: this.sequence++,
      });
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

  private loadOne(hash: string) {
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

        this.loading.delete(hash);
        this.bitmaps.set(hash, bitmap);
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
