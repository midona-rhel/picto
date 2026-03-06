// Legacy compatibility wrapper around shared media QoS scheduler.
// New code should use `mediaQosScheduler` directly.

import { enqueueMediaQosTask, type MediaQosTaskHandle } from './mediaQosScheduler';

type LoadCallback = (signal: AbortSignal) => void | Promise<void>;

interface ActiveEntry {
  handle: MediaQosTaskHandle;
  resolvePending?: () => void;
}

const activeUrls = new Map<string, ActiveEntry>();

function isHeavyDecodeUrl(url: string): boolean {
  const lower = url.toLowerCase();
  return lower.endsWith('.webp') || lower.endsWith('.avif');
}

/** Queue an image load. Callback fires when a slot is available. */
export function enqueue(url: string, priority: number, callback: LoadCallback) {
  // De-duplicate by URL for legacy callers.
  if (activeUrls.has(url)) {
    return;
  }

  let settledExternally = false;
  let externalResolve: (() => void) | undefined;

  const handle = enqueueMediaQosTask({
    lane: priority <= 0 ? 'visible' : 'prefetch',
    priority,
    heavy: isHeavyDecodeUrl(url),
    run: async (signal) => {
      const maybePromise = callback(signal);
      if (maybePromise && typeof (maybePromise as PromiseLike<unknown>).then === 'function') {
        await (maybePromise as PromiseLike<unknown>);
        return;
      }
      await new Promise<void>((resolve) => {
        if (settledExternally) {
          resolve();
          return;
        }
        externalResolve = resolve;
      });
    },
  });

  activeUrls.set(url, {
    handle,
    resolvePending: () => {
      settledExternally = true;
      if (externalResolve) {
        const resolve = externalResolve;
        externalResolve = undefined;
        resolve();
      }
    },
  });
}

/** Signal that a load finished — frees one slot and processes the queue. */
export function complete(url: string) {
  const entry = activeUrls.get(url);
  if (!entry) return;
  entry.resolvePending?.();
  activeUrls.delete(url);
}

/** Cancel all pending and active loads. */
export function cancelAll() {
  for (const [, entry] of activeUrls) {
    entry.handle.cancel();
    entry.resolvePending?.();
  }
  activeUrls.clear();
}

/** Cancel a pending or active load -- frees the slot if active. */
export function cancel(url: string) {
  const entry = activeUrls.get(url);
  if (!entry) return;
  entry.handle.cancel();
  entry.resolvePending?.();
  activeUrls.delete(url);
}
