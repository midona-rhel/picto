import ThumbnailWorker from './thumbnailDecodeWorker?worker';

type BitmapCallback = (hash: string, bitmap: ImageBitmap) => void;

let worker: Worker | null = null;
let bitmapCb: BitmapCallback | null = null;

function ensureWorker(): Worker | null {
  if (worker) return worker;
  try {
    worker = new ThumbnailWorker();
    worker.onmessage = (event: MessageEvent) => {
      const msg = event.data;
      if (msg.type === 'bitmap') bitmapCb?.(msg.hash, msg.bitmap);
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

export function sendThumbnailPlan(entries: Array<{ hash: string; url: string }>): void {
  ensureWorker()?.postMessage({ type: 'plan', entries });
}

export function clearThumbnailWorker(): void {
  worker?.postMessage({ type: 'clear' });
}

export function setThumbnailBitmapCallback(cb: BitmapCallback | null): void {
  bitmapCb = cb;
}
