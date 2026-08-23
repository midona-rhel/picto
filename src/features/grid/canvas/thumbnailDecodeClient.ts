/**
 * Thin bridge to the thumbnail decode worker.
 *
 * The main thread sends an activation plan and receives decoded bitmaps.
 */

import ThumbnailWorker from './thumbnailDecodeWorker?worker';

type BitmapCallback = (hash: string, bitmap: ImageBitmap) => void;
type ErrorCallback = (hash: string) => void;

let worker: Worker | null = null;
let bitmapCb: BitmapCallback | null = null;
let errorCb: ErrorCallback | null = null;

function ensureWorker(): Worker | null {
  if (worker) return worker;
  try {
    worker = new ThumbnailWorker();
    worker.onmessage = (event: MessageEvent) => {
      const msg = event.data;
      if (msg.type === 'bitmap') bitmapCb?.(msg.hash, msg.bitmap);
      else if (msg.type === 'error') errorCb?.(msg.hash);
    };
    worker.onerror = () => {
      worker?.terminate();
      worker = null;
    };
  } catch {
    worker = null;
  }
  return worker;
}

/** Send the current set of visible tiles to the worker. */
export function sendThumbnailPlan(entries: Array<{ hash: string; url: string }>): void {
  ensureWorker()?.postMessage({ type: 'plan', entries });
}

/** Clear all worker state (scope change). */
export function clearThumbnailWorker(): void {
  worker?.postMessage({ type: 'clear' });
}

export function setThumbnailBitmapCallback(cb: BitmapCallback | null): void {
  bitmapCb = cb;
}

export function setThumbnailErrorCallback(cb: ErrorCallback | null): void {
  errorCb = cb;
}

export function terminateThumbnailWorker(): void {
  worker?.terminate();
  worker = null;
}
