/**
 * Thin bridge to the thumbnail decode worker.
 *
 * The worker owns all loading, caching, and reveal staggering.
 * The main thread sends a plan (visible hashes) and receives
 * revealed bitmaps — it never waits on the worker.
 */

import ThumbnailWorker from './thumbnailDecodeWorker?worker';

type RevealCallback = (hash: string, bitmap: ImageBitmap) => void;
type ErrorCallback = (hash: string) => void;

let worker: Worker | null = null;
let revealCb: RevealCallback | null = null;
let errorCb: ErrorCallback | null = null;

function ensureWorker(): Worker | null {
  if (worker) return worker;
  try {
    worker = new ThumbnailWorker();
    worker.onmessage = (event: MessageEvent) => {
      const msg = event.data;
      if (msg.type === 'reveal') revealCb?.(msg.hash, msg.bitmap);
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

export function setThumbnailRevealCallback(cb: RevealCallback | null): void {
  revealCb = cb;
}

export function setThumbnailErrorCallback(cb: ErrorCallback | null): void {
  errorCb = cb;
}

export function terminateThumbnailWorker(): void {
  worker?.terminate();
  worker = null;
}
