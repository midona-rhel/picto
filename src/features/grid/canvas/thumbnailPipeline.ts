/**
 * Thumbnail pipeline — loads, caches, and evicts ImageBitmaps for the canvas grid.
 *
 * Visible items are loaded immediately. Prefetch items are queued.
 * Items outside the keep zone are evicted to manage memory.
 */

export type OnThumbnailLoaded = (hash: string) => void;

export class ThumbnailPipeline {
  private bitmaps = new Map<string, ImageBitmap>();
  private loading = new Set<string>();
  private onLoaded: OnThumbnailLoaded;

  constructor(onLoaded: OnThumbnailLoaded) {
    this.onLoaded = onLoaded;
  }

  get(hash: string): ImageBitmap | undefined {
    return this.bitmaps.get(hash);
  }

  getAll(): Map<string, ImageBitmap> {
    return this.bitmaps;
  }

  /** Request thumbnails for the given hashes. Already loaded/loading hashes are skipped. */
  request(hashes: string[]) {
    for (const hash of hashes) {
      if (this.bitmaps.has(hash) || this.loading.has(hash)) continue;
      this.loading.add(hash);
      this.loadOne(hash);
    }
  }

  /** Evict bitmaps not in the keep set. */
  evict(keepHashes: Set<string>, maxEvictPerTick = 5) {
    let evicted = 0;
    for (const [hash, bitmap] of this.bitmaps) {
      if (evicted >= maxEvictPerTick) break;
      if (!keepHashes.has(hash)) {
        bitmap.close();
        this.bitmaps.delete(hash);
        evicted++;
      }
    }
  }

  /** Cancel all in-flight loads and clear all bitmaps. */
  clear() {
    this.loading.clear();
    for (const bitmap of this.bitmaps.values()) {
      bitmap.close();
    }
    this.bitmaps.clear();
  }

  private async loadOne(hash: string) {
    try {
      const url = `media://host/thumb/${hash}.jpg`;
      const response = await fetch(url);
      if (!response.ok) {
        this.loading.delete(hash);
        return;
      }
      const blob = await response.blob();
      const bitmap = await createImageBitmap(blob);

      // Check if still wanted (not evicted/cleared while loading)
      if (!this.loading.has(hash)) {
        bitmap.close();
        return;
      }

      this.loading.delete(hash);
      this.bitmaps.set(hash, bitmap);
      this.onLoaded(hash);
    } catch {
      this.loading.delete(hash);
    }
  }
}
