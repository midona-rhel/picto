/// <reference lib="webworker" />

/**
 * Thumbnail decode worker — owns loading, cancellation, and decoding.
 *
 * The main thread sends a "plan" (visible hashes + URLs) each frame.
 * Decoded bitmaps are transferred immediately. Reveal timing belongs to the
 * main-thread ThumbnailRevealTracker.
 */

// ── Messages ────────────────────────────────────────────────────

type PlanEntry = { fileHash: string; url: string };
type PlanMessage = { type: 'plan'; entries: PlanEntry[] };
type ClearMessage = { type: 'clear' };
type InvalidateMessage = { type: 'invalidate'; fileHash: string };
type IncomingMessage = PlanMessage | ClearMessage | InvalidateMessage;

// ── State ───────────────────────────────────────────────────────

const ctx = self as DedicatedWorkerGlobalScope;

const currentPlan = new Map<string, string>();          // file hash → url
const inFlight = new Map<string, AbortController>();
const delivered = new Map<string, string>();             // hash -> transferred URL
const failCounts = new Map<string, number>();

const MAX_CONCURRENT = 6;
const MAX_FAILURES = 2;

// ── Plan ────────────────────────────────────────────────────────

function handlePlan(entries: PlanEntry[]): void {
  // Build next plan as a set for O(1) lookups
  const nextFileHashes = new Set<string>();
  const nextUrls = new Map<string, string>();
  for (let i = 0; i < entries.length; i++) {
    nextFileHashes.add(entries[i].fileHash);
    nextUrls.set(entries[i].fileHash, entries[i].url);
  }

  // Cancel loads no longer in plan
  for (const [fileHash, controller] of inFlight) {
    if (!nextFileHashes.has(fileHash) || currentPlan.get(fileHash) !== nextUrls.get(fileHash)) {
      controller.abort();
      inFlight.delete(fileHash);
    }
  }

  for (const [fileHash, url] of delivered) {
    if (!nextFileHashes.has(fileHash) || nextUrls.get(fileHash) !== url) delivered.delete(fileHash);
  }

  currentPlan.clear();
  for (const [h, u] of nextUrls) currentPlan.set(h, u);

  pumpLoads();
}

function handleClear(): void {
  for (const c of inFlight.values()) c.abort();
  inFlight.clear();
  delivered.clear();
  failCounts.clear();
  currentPlan.clear();
}

function handleInvalidate(fileHash: string): void {
  inFlight.get(fileHash)?.abort();
  inFlight.delete(fileHash);
  delivered.delete(fileHash);
  failCounts.delete(fileHash);
}

// ── Loading ─────────────────────────────────────────────────────

function pumpLoads(): void {
  for (const [fileHash, url] of currentPlan) {
    if (inFlight.size >= MAX_CONCURRENT) break;
    if (inFlight.has(fileHash) || delivered.has(fileHash)) continue;
    if ((failCounts.get(fileHash) ?? 0) >= MAX_FAILURES) continue;
    startLoad(fileHash, url);
  }
}

function startLoad(fileHash: string, url: string): void {
  const controller = new AbortController();
  inFlight.set(fileHash, controller);

  void (async () => {
    try {
      const response = await fetch(url, { signal: controller.signal });
      if (!response.ok) throw new Error(`fetch ${response.status}`);
      const blob = await response.blob();
      const bitmap = await createImageBitmap(blob);

      if (controller.signal.aborted) { bitmap.close(); return; }
      inFlight.delete(fileHash);

      if (!currentPlan.has(fileHash)) { bitmap.close(); return; }

      delivered.set(fileHash, url);
      ctx.postMessage({ type: 'bitmap', fileHash, bitmap }, [bitmap]);
    } catch (error) {
      inFlight.delete(fileHash);
      if ((error as Error)?.name === 'AbortError') return;
      failCounts.set(fileHash, (failCounts.get(fileHash) ?? 0) + 1);
      ctx.postMessage({ type: 'error', fileHash });
    } finally {
      pumpLoads();
    }
  })();
}

// ── Entry ───────────────────────────────────────────────────────

ctx.onmessage = (event: MessageEvent<IncomingMessage>) => {
  const msg = event.data;
  if (msg.type === 'plan') handlePlan(msg.entries);
  else if (msg.type === 'clear') handleClear();
  else if (msg.type === 'invalidate') handleInvalidate(msg.fileHash);
};
