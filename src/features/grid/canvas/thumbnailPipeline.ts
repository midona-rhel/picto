// Main-thread bitmap residency; reveal identity remains in ThumbnailRevealTracker.
import {
  sendThumbnailPlan,
  clearThumbnailWorker,
  setThumbnailBitmapCallback,
} from './thumbnailDecodeClient';
import { mediaFileUrl } from '../../../shared/lib/mediaUrl';
import type { ThumbnailPipelineEntry } from './thumbnailPipelineTypes';
import {
  computePlanFingerprint,
  sortPlanTilesByViewportDistance,
  type PlanTile,
} from './thumbnailPlan';

export type { PlanTile } from './thumbnailPlan';

export type { ThumbnailPipelineEntry } from './thumbnailPipelineTypes';

const FULL_QUALITY_THRESHOLD_PX = 752;

function mediaThumbnailUrl(hash: string): string {
  return `media://localhost/thumb/${hash}.jpg`;
}

export class ThumbnailPipeline {
  private cache = new Map<string, ThumbnailPipelineEntry>();
  private onDirty: () => void;
  private onBitmapAvailable: (hash: string) => void;
  private destroyed = false;

  private lastPlanFingerprint = -1;
  private planBuffer: Array<{ hash: string; url: string }> = [];

  constructor(onDirty: () => void = () => {}, onBitmapAvailable: (hash: string) => void = () => {}) {
    this.onDirty = onDirty;
    this.onBitmapAvailable = onBitmapAvailable;
    setThumbnailBitmapCallback((hash, bitmap) => this.handleBitmap(hash, bitmap));
  }

  updatePlan(tiles: PlanTile[], viewportCenterY: number): void {
    if (this.destroyed) return;

    // Layout order keeps the fingerprint stable while viewport distance changes.
    const fingerprint = computePlanFingerprint(tiles, FULL_QUALITY_THRESHOLD_PX);
    if (fingerprint === this.lastPlanFingerprint) return;
    this.lastPlanFingerprint = fingerprint;

    // The caller rewrites its reusable tile buffer next frame.
    sortPlanTilesByViewportDistance(tiles, viewportCenterY);

    const buf = this.planBuffer;
    buf.length = tiles.length;
    for (let i = 0; i < tiles.length; i++) {
      const t = tiles[i];
      const isImage = t.mime.startsWith('image/') && t.mime !== 'image/gif';
      const needsFull = isImage && (t.w > FULL_QUALITY_THRESHOLD_PX || t.h > FULL_QUALITY_THRESHOLD_PX);
      const url = needsFull ? mediaFileUrl(t.hash, t.mime) : mediaThumbnailUrl(t.hash);
      if (buf[i]) {
        buf[i].hash = t.hash;
        buf[i].url = url;
      } else {
        buf[i] = { hash: t.hash, url };
      }
    }
    sendThumbnailPlan(buf);
  }

  get(hash: string): ThumbnailPipelineEntry | null {
    return this.cache.get(hash) ?? null;
  }

  evictOutsideActive(activeHashes: Set<string>): void {
    for (const [hash, entry] of this.cache) {
      if (activeHashes.has(hash)) continue;
      entry.thumb?.close();
      this.cache.delete(hash);
    }
  }

  clear(): void {
    this.destroyed = true;
    this.lastPlanFingerprint = -1;
    clearThumbnailWorker();
    for (const entry of this.cache.values()) entry.thumb?.close();
    this.cache.clear();
  }

  private handleBitmap(hash: string, bitmap: ImageBitmap): void {
    if (this.destroyed) { bitmap.close(); return; }

    let entry = this.cache.get(hash);
    if (!entry) {
      entry = { thumb: null };
      this.cache.set(hash, entry);
    }

    const isUpgrade = entry.thumb != null;
    entry.thumb?.close();

    entry.thumb = bitmap;

    if (!isUpgrade) this.onBitmapAvailable(hash);
    this.onDirty();
  }
}
