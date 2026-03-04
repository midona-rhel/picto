// Concurrent load queue: max 6 active loads, priority-sorted.
// Avoids saturating the browser's connection pool during fast scrolling.

type LoadCallback = (signal: AbortSignal) => void;

interface QueueEntry {
  url: string;
  priority: number; // lower = higher priority (0 = thumb, 1 = full)
  callback: LoadCallback;
}

const MAX_CONCURRENT = 6;
let active = 0;
const queue: QueueEntry[] = [];
const activeUrls = new Map<string, AbortController>();
const pendingUrls = new Set<string>();

function processQueue() {
  while (active < MAX_CONCURRENT && queue.length > 0) {
    queue.sort((a, b) => a.priority - b.priority);
    const entry = queue.shift()!;
    pendingUrls.delete(entry.url);
    active++;
    const controller = new AbortController();
    activeUrls.set(entry.url, controller);
    entry.callback(controller.signal);
  }
}

/** Queue an image load. Callback fires when a slot is available. */
export function enqueue(url: string, priority: number, callback: LoadCallback) {
  if (active < MAX_CONCURRENT) {
    active++;
    const controller = new AbortController();
    activeUrls.set(url, controller);
    callback(controller.signal);
    return;
  }
  pendingUrls.add(url);
  queue.push({ url, priority, callback });
}

/** Signal that a load finished — frees one slot and processes the queue. */
export function complete(url: string) {
  if (activeUrls.delete(url)) {
    active = Math.max(0, active - 1);
    processQueue();
  }
}

/** Cancel all pending and active loads. */
export function cancelAll() {
  for (const entry of queue) pendingUrls.delete(entry.url);
  queue.length = 0;
  for (const [, controller] of activeUrls) {
    controller.abort();
  }
  activeUrls.clear();
  active = 0;
}

/** Cancel a pending or active load -- frees the slot if active. */
export function cancel(url: string) {
  if (pendingUrls.delete(url)) {
    const idx = queue.findIndex((e) => e.url === url);
    if (idx >= 0) queue.splice(idx, 1);
    return;
  }
  const controller = activeUrls.get(url);
  if (controller) {
    controller.abort();
    activeUrls.delete(url);
    active = Math.max(0, active - 1);
    processQueue();
  }
}
