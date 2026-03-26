import ThumbnailDecodeWorker from './thumbnailDecodeWorker?worker';

interface WorkerSuccessResponse {
  type: 'success';
  requestId: number;
  durationMs: number;
  bitmap: ImageBitmap;
}

interface WorkerErrorResponse {
  type: 'error';
  requestId: number;
  error: string;
}

type WorkerResponse = WorkerSuccessResponse | WorkerErrorResponse;

interface PendingRequest {
  resolve: (value: { bitmap: ImageBitmap; durationMs: number }) => void;
  reject: (reason?: unknown) => void;
  meta?: Record<string, unknown>;
}

interface WorkerSlot {
  worker: Worker;
  pending: Map<number, PendingRequest>;
  aborted: Map<number, Record<string, unknown> | undefined>;
}

const WORKER_POOL_SIZE = 2;

let workerSlots: WorkerSlot[] | null | undefined;
let nextRequestId = 1;
let droppedLateResponses = 0;
let lateResponseListener: ((meta?: Record<string, unknown>) => void) | null = null;

function destroyPool(): void {
  if (!workerSlots) return;
  for (const slot of workerSlots) {
    for (const request of slot.pending.values()) {
      request.reject(new Error('thumbnail worker failed'));
    }
    slot.pending.clear();
    slot.worker.terminate();
  }
  workerSlots = undefined;
}

function createWorkerSlot(): WorkerSlot {
  const worker = new ThumbnailDecodeWorker();
  const slot: WorkerSlot = {
    worker,
    pending: new Map(),
    aborted: new Map(),
  };

  worker.onmessage = (event: MessageEvent<WorkerResponse>) => {
    const message = event.data;
    const request = slot.pending.get(message.requestId);
    if (!request) {
      const meta = slot.aborted.get(message.requestId);
      slot.aborted.delete(message.requestId);
      if (message.type === 'success') {
        message.bitmap.close();
      }
      droppedLateResponses += 1;
      lateResponseListener?.(meta);
      return;
    }

    slot.pending.delete(message.requestId);
    slot.aborted.delete(message.requestId);
    if (message.type === 'success') {
      request.resolve({ bitmap: message.bitmap, durationMs: message.durationMs });
      return;
    }
    request.reject(new Error(message.error));
  };

  worker.onerror = () => {
    destroyPool();
  };

  return slot;
}

function ensureWorkerPool(): WorkerSlot[] | null {
  if (workerSlots !== undefined) return workerSlots;
  try {
    workerSlots = Array.from({ length: WORKER_POOL_SIZE }, () => createWorkerSlot());
  } catch {
    destroyPool();
    workerSlots = null;
  }
  return workerSlots;
}

function pickWorkerSlot(slots: WorkerSlot[]): WorkerSlot {
  return slots.reduce((best, candidate) => (
    candidate.pending.size < best.pending.size ? candidate : best
  ), slots[0]);
}

export function decodeThumbnailInWorker(
  url: string,
  signal: AbortSignal,
  meta?: Record<string, unknown>,
): Promise<{ bitmap: ImageBitmap; durationMs: number }> | null {
  const slots = ensureWorkerPool();
  if (!slots || typeof createImageBitmap !== 'function') return null;

  const slot = pickWorkerSlot(slots);
  const requestId = nextRequestId++;
  let abort: (() => void) | null = null;

  const promise = new Promise<{ bitmap: ImageBitmap; durationMs: number }>((resolve, reject) => {
    abort = () => {
      const request = slot.pending.get(requestId);
      const removed = slot.pending.delete(requestId);
      if (removed) {
        slot.aborted.set(requestId, request?.meta);
      }
      slot.worker.postMessage({ type: 'cancel', requestId });
      if (removed) {
        reject(new DOMException('Aborted', 'AbortError'));
      }
    };

    if (signal.aborted) {
      abort();
      return;
    }

    slot.pending.set(requestId, { resolve, reject, meta });
    signal.addEventListener('abort', abort, { once: true });
    slot.worker.postMessage({
      type: 'decode',
      requestId,
      url,
    });
  });

  return promise.finally(() => {
    if (abort) signal.removeEventListener('abort', abort);
  });
}

export function getThumbnailDecodeWorkerStats() {
  const slots = workerSlots ?? [];
  return {
    poolSize: Array.isArray(workerSlots) ? workerSlots.length : 0,
    pendingRequests: slots.reduce((sum, slot) => sum + slot.pending.size, 0),
    droppedLateResponses,
  };
}

export function setThumbnailDecodeLateResponseListener(
  listener: ((meta?: Record<string, unknown>) => void) | null,
): void {
  lateResponseListener = listener;
}
