import ThumbnailDecodeWorker from './thumbnailDecodeWorker?worker';
import type { ThumbnailQueueItem, ThumbnailSourceKind } from './thumbnailPipelineTypes';

interface WorkerSuccessResponse {
  type: 'success';
  requestId: number;
  sourceKind: ThumbnailSourceKind;
  loadedLongEdge: number;
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
  resolve: (value: {
    bitmap: ImageBitmap;
    sourceKind: ThumbnailSourceKind;
    loadedLongEdge: number;
    durationMs: number;
  }) => void;
  reject: (reason?: unknown) => void;
}

interface WorkerSlot {
  worker: Worker;
  pending: Map<number, PendingRequest>;
}

const WORKER_POOL_SIZE = 2;

let workerSlots: WorkerSlot[] | null | undefined;
let nextRequestId = 1;
let droppedLateResponses = 0;

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
  };

  worker.onmessage = (event: MessageEvent<WorkerResponse>) => {
    const message = event.data;
    const request = slot.pending.get(message.requestId);
    if (!request) {
      if (message.type === 'success') {
        message.bitmap.close();
      }
      droppedLateResponses += 1;
      return;
    }

    slot.pending.delete(message.requestId);
    if (message.type === 'success') {
      request.resolve({
        bitmap: message.bitmap,
        sourceKind: message.sourceKind,
        loadedLongEdge: message.loadedLongEdge,
        durationMs: message.durationMs,
      });
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
  item: ThumbnailQueueItem,
  signal: AbortSignal,
): Promise<{
  bitmap: ImageBitmap;
  sourceKind: ThumbnailSourceKind;
  loadedLongEdge: number;
  durationMs: number;
}> | null {
  const slots = ensureWorkerPool();
  if (!slots || typeof createImageBitmap !== 'function') return null;

  const slot = pickWorkerSlot(slots);
  const requestId = nextRequestId++;
  let abort: (() => void) | null = null;

  const promise = new Promise<{
    bitmap: ImageBitmap;
    sourceKind: ThumbnailSourceKind;
    loadedLongEdge: number;
    durationMs: number;
  }>((resolve, reject) => {
    abort = () => {
      const removed = slot.pending.delete(requestId);
      slot.worker.postMessage({ type: 'cancel', requestId });
      if (removed) {
        reject(new DOMException('Aborted', 'AbortError'));
      }
    };

    if (signal.aborted) {
      abort();
      return;
    }

    slot.pending.set(requestId, { resolve, reject });
    signal.addEventListener('abort', abort, { once: true });

    slot.worker.postMessage({
      type: 'decode',
      requestId,
      url: item.url,
      sourceKind: item.sourceKind,
      loadedLongEdge: item.requestedLongEdge,
      resizeWidth: item.resizeWidth,
      resizeHeight: item.resizeHeight,
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

export function resetThumbnailDecodeWorkerForTests(): void {
  if (workerSlots) {
    for (const slot of workerSlots) {
      slot.worker.terminate();
      slot.pending.clear();
    }
  }
  workerSlots = undefined;
  nextRequestId = 1;
  droppedLateResponses = 0;
}
