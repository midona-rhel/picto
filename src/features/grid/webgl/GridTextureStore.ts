import { Texture } from './pixiRuntime';

export type TextureStatus = 'loading' | 'ready' | 'failed';

export interface TextureEntry {
  hash: string;
  url: string;
  status: TextureStatus;
  texture: Texture | null;
  image: HTMLImageElement | null;
  lastUsedAt: number;
  byteSize: number;
  error: Error | null;
}

interface GridTextureStoreOptions {
  byteBudget: number;
  onChange?: () => void;
}

interface PendingUpload {
  entry: TextureEntry;
  bitmap: ImageBitmap;
}

const UPLOAD_BUDGET_MS = 3;
const MAX_UPLOAD_QUEUE = 20;

export class GridTextureStore {
  private readonly onChange?: () => void;
  private readonly byteBudget: number;
  private readonly entries = new Map<string, TextureEntry>();
  private readonly uploadQueue: PendingUpload[] = [];
  private totalBytes = 0;
  private flushHandle: number | null = null;

  constructor(options: GridTextureStoreOptions) {
    this.onChange = options.onChange;
    this.byteBudget = options.byteBudget;
  }

  ensure(hash: string, url: string): TextureEntry {
    const now = performance.now();
    const existing = this.entries.get(hash);
    if (existing) {
      existing.lastUsedAt = now;
      if (existing.url !== url && existing.status !== 'loading') {
        existing.url = url;
      }
      return existing;
    }

    const entry: TextureEntry = {
      hash,
      url,
      status: 'loading',
      texture: null,
      image: null,
      lastUsedAt: now,
      byteSize: 0,
      error: null,
    };
    this.entries.set(hash, entry);
    this.load(entry);
    return entry;
  }

  get(hash: string): TextureEntry | null {
    return this.entries.get(hash) ?? null;
  }

  sweep(retained: Set<string>): void {
    for (const [hash, entry] of this.entries) {
      if (retained.has(hash)) continue;
      if (entry.status === 'loading') {
        this.cancelEntry(entry);
        this.entries.delete(hash);
      }
    }
    this.evictOverBudget(retained);
  }

  destroy(): void {
    if (this.flushHandle != null) {
      cancelAnimationFrame(this.flushHandle);
      this.flushHandle = null;
    }
    for (const pending of this.uploadQueue) {
      pending.bitmap.close();
    }
    this.uploadQueue.length = 0;
    for (const entry of this.entries.values()) {
      this.cancelEntry(entry);
      entry.texture?.destroy(true);
    }
    this.entries.clear();
    this.totalBytes = 0;
  }

  private load(entry: TextureEntry): void {
    const image = new Image();
    image.crossOrigin = 'anonymous';
    image.decoding = 'async';
    entry.image = image;

    image.onload = () => {
      if (this.entries.get(entry.hash) !== entry) return;
      entry.image = null;
      createImageBitmap(image).then((bitmap) => {
        if (this.entries.get(entry.hash) !== entry) {
          bitmap.close();
          return;
        }
        if (this.uploadQueue.length >= MAX_UPLOAD_QUEUE) {
          const dropped = this.uploadQueue.shift()!;
          dropped.bitmap.close();
        }
        this.uploadQueue.push({ entry, bitmap });
        this.scheduleFlush();
      }).catch(() => {
        this.uploadSync(entry, image);
      });
    };

    image.onerror = () => {
      if (this.entries.get(entry.hash) !== entry) return;
      entry.status = 'failed';
      entry.image = null;
      entry.error = new Error('image load failed');
      entry.lastUsedAt = performance.now();
      console.error('[grid-webgl] image load failed', {
        hash: entry.hash,
        url: entry.url,
        error: entry.error,
      });
      this.onChange?.();
    };

    image.src = entry.url;
  }

  private uploadSync(entry: TextureEntry, source: HTMLImageElement | ImageBitmap): void {
    if (this.entries.get(entry.hash) !== entry) return;
    try {
      const texture = Texture.from(source);
      if (this.entries.get(entry.hash) !== entry) {
        texture.destroy(true);
        return;
      }
      const w = texture.width || ('naturalWidth' in source ? source.naturalWidth : source.width) || 1;
      const h = texture.height || ('naturalHeight' in source ? source.naturalHeight : source.height) || 1;
      entry.texture = texture;
      entry.status = 'ready';
      entry.byteSize = Math.max(1, w * h * 4);
      entry.lastUsedAt = performance.now();
      this.totalBytes += entry.byteSize;
    } catch (error) {
      entry.status = 'failed';
      entry.error = error instanceof Error ? error : new Error(String(error));
      entry.lastUsedAt = performance.now();
      console.error('[grid-webgl] texture upload failed', {
        hash: entry.hash,
        url: entry.url,
        error: entry.error,
      });
    }
    this.onChange?.();
  }

  private scheduleFlush(): void {
    if (this.flushHandle != null) return;
    this.flushHandle = requestAnimationFrame(() => {
      this.flushHandle = null;
      this.flushUploadQueue();
    });
  }

  private flushUploadQueue(): void {
    const start = performance.now();
    let uploaded = false;
    while (this.uploadQueue.length > 0) {
      if (performance.now() - start > UPLOAD_BUDGET_MS) {
        this.scheduleFlush();
        break;
      }
      const { entry, bitmap } = this.uploadQueue.shift()!;
      if (this.entries.get(entry.hash) !== entry) {
        bitmap.close();
        continue;
      }
      try {
        const texture = Texture.from(bitmap);
        if (this.entries.get(entry.hash) !== entry) {
          texture.destroy(true);
          bitmap.close();
          continue;
        }
        entry.texture = texture;
        entry.status = 'ready';
        entry.byteSize = Math.max(1, bitmap.width * bitmap.height * 4);
        entry.lastUsedAt = performance.now();
        this.totalBytes += entry.byteSize;
        uploaded = true;
      } catch (error) {
        bitmap.close();
        entry.status = 'failed';
        entry.error = error instanceof Error ? error : new Error(String(error));
        entry.lastUsedAt = performance.now();
        uploaded = true;
      }
    }
    if (uploaded) {
      this.evictOverBudget();
      this.onChange?.();
    }
  }

  private evictOverBudget(retained?: Set<string>): void {
    if (this.totalBytes <= this.byteBudget) return;
    const candidates: TextureEntry[] = [];
    for (const [hash, entry] of this.entries) {
      if (retained?.has(hash)) continue;
      if (entry.status === 'ready' && entry.texture) {
        candidates.push(entry);
      }
    }
    candidates.sort((a, b) => a.lastUsedAt - b.lastUsedAt);
    for (const entry of candidates) {
      if (this.totalBytes <= this.byteBudget) break;
      this.totalBytes -= entry.byteSize;
      entry.texture!.destroy(true);
      entry.texture = null;
      this.entries.delete(entry.hash);
    }
  }

  private cancelEntry(entry: TextureEntry): void {
    const image = entry.image;
    if (!image) return;
    image.onload = null;
    image.onerror = null;
    image.src = '';
    entry.image = null;
  }
}
