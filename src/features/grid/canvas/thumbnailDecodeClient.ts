/** One decode-worker bridge per canvas grid. */

import ThumbnailWorker from './thumbnailDecodeWorker?worker';

type BitmapCallback = (fileHash: string, bitmap: ImageBitmap) => void;
type ErrorCallback = (fileHash: string) => void;

type WorkerFactory = () => Worker;

export class ThumbnailDecodeClient {
  private worker: Worker | null = null;

  constructor(
    private readonly onBitmap: BitmapCallback,
    private readonly onError: ErrorCallback,
    private readonly createWorker: WorkerFactory = () => new ThumbnailWorker(),
  ) {}

  private ensureWorker(): Worker | null {
    if (this.worker) return this.worker;
    try {
      const worker = this.createWorker();
      worker.onmessage = (event: MessageEvent) => {
        const message = event.data;
        if (message.type === 'bitmap') this.onBitmap(message.fileHash, message.bitmap);
        else if (message.type === 'error') this.onError(message.fileHash);
      };
      worker.onerror = () => {
        worker.terminate();
        if (this.worker === worker) this.worker = null;
      };
      this.worker = worker;
      return worker;
    } catch {
      return null;
    }
  }

  sendPlan(entries: Array<{ fileHash: string; url: string }>): void {
    this.ensureWorker()?.postMessage({ type: 'plan', entries });
  }

  invalidate(fileHash: string): void {
    this.ensureWorker()?.postMessage({ type: 'invalidate', fileHash });
  }

  clear(): void {
    this.worker?.postMessage({ type: 'clear' });
  }

  terminate(): void {
    this.worker?.terminate();
    this.worker = null;
  }
}
