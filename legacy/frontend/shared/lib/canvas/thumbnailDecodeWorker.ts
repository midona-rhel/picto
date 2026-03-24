type DecodeMessage = {
  type: 'decode';
  requestId: number;
  url: string;
  sourceKind: 'thumbnail' | 'full';
  loadedLongEdge: number;
  resizeWidth?: number;
  resizeHeight?: number;
};

type CancelMessage = {
  type: 'cancel';
  requestId: number;
};

type WorkerMessage = DecodeMessage | CancelMessage;

type SuccessResponse = {
  type: 'success';
  requestId: number;
  sourceKind: 'thumbnail' | 'full';
  loadedLongEdge: number;
  durationMs: number;
  bitmap: ImageBitmap;
};

type ErrorResponse = {
  type: 'error';
  requestId: number;
  error: string;
};

const worker = self as DedicatedWorkerGlobalScope;
const controllers = new Map<number, AbortController>();

async function decodeRequest(message: DecodeMessage): Promise<void> {
  const controller = new AbortController();
  controllers.set(message.requestId, controller);
  const startedAt = performance.now();

  try {
    const response = await fetch(message.url, { signal: controller.signal });
    if (!response.ok) throw new Error(`thumbnail fetch failed: ${response.status}`);
    const blob = await response.blob();

    const bitmap = message.sourceKind === 'full' && message.resizeWidth && message.resizeHeight
      ? await createImageBitmap(blob, {
        resizeWidth: message.resizeWidth,
        resizeHeight: message.resizeHeight,
        resizeQuality: 'medium',
      })
      : await createImageBitmap(blob);

    if (controller.signal.aborted) {
      bitmap.close();
      return;
    }

    const result: SuccessResponse = {
      type: 'success',
      requestId: message.requestId,
      sourceKind: message.sourceKind,
      loadedLongEdge: message.loadedLongEdge,
      durationMs: performance.now() - startedAt,
      bitmap,
    };
    worker.postMessage(result, [bitmap]);
  } catch (error) {
    if (controller.signal.aborted) return;
    const result: ErrorResponse = {
      type: 'error',
      requestId: message.requestId,
      error: error instanceof Error ? error.message : String(error),
    };
    worker.postMessage(result);
  } finally {
    controllers.delete(message.requestId);
  }
}

worker.onmessage = (event: MessageEvent<WorkerMessage>) => {
  const message = event.data;
  if (message.type === 'cancel') {
    controllers.get(message.requestId)?.abort();
    controllers.delete(message.requestId);
    return;
  }
  void decodeRequest(message);
};
