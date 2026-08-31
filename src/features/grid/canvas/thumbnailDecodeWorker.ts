/// <reference lib="webworker" />

/**
 * Thumbnail decode worker — owns loading, cancellation, and decoding.
 *
 * The main thread sends a "plan" (visible hashes + URLs) each frame.
 * Decoded bitmaps are transferred immediately. Reveal timing belongs to the
 * main-thread ThumbnailRevealTracker.
 */

// ── Messages ────────────────────────────────────────────────────

type DecodeQuality = 'thumbnail' | 'full';
type PlanEntry = { fileHash: string; url: string; quality: DecodeQuality };
type PlanMessage = { type: 'plan'; entries: PlanEntry[] };
type ClearMessage = { type: 'clear' };
type InvalidateMessage = { type: 'invalidate'; fileHash: string };
type IncomingMessage = PlanMessage | ClearMessage | InvalidateMessage;

// ── State ───────────────────────────────────────────────────────

const ctx = self as DedicatedWorkerGlobalScope;

const currentPlan = new Map<string, PlanEntry>();
const inFlight = new Map<string, AbortController>();
const delivered = new Map<string, string>();             // hash -> transferred URL
const failCounts = new Map<string, number>();

const MAX_CONCURRENT = 6;
const MAX_FAILURES = 2;

// ── Plan ────────────────────────────────────────────────────────

function handlePlan(entries: PlanEntry[]): void {
  // Build next plan as a set for O(1) lookups
  const nextFileHashes = new Set<string>();
  const nextEntries = new Map<string, PlanEntry>();
  for (let i = 0; i < entries.length; i++) {
    nextFileHashes.add(entries[i].fileHash);
    nextEntries.set(entries[i].fileHash, entries[i]);
  }

  // Cancel loads no longer in plan
  for (const [fileHash, controller] of inFlight) {
    if (!nextFileHashes.has(fileHash) || currentPlan.get(fileHash)?.url !== nextEntries.get(fileHash)?.url) {
      controller.abort();
      inFlight.delete(fileHash);
    }
  }

  for (const [fileHash, url] of delivered) {
    if (!nextFileHashes.has(fileHash) || nextEntries.get(fileHash)?.url !== url) delivered.delete(fileHash);
  }

  currentPlan.clear();
  for (const [fileHash, entry] of nextEntries) currentPlan.set(fileHash, entry);

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
  for (const [fileHash, entry] of currentPlan) {
    if (inFlight.size >= MAX_CONCURRENT) break;
    if (inFlight.has(fileHash) || delivered.has(fileHash)) continue;
    if ((failCounts.get(fileHash) ?? 0) >= MAX_FAILURES) continue;
    startLoad(entry);
  }
}

function startLoad(entry: PlanEntry): void {
  const { fileHash, url, quality } = entry;
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

      if (currentPlan.get(fileHash)?.url !== url) { bitmap.close(); return; }

      delivered.set(fileHash, url);
      ctx.postMessage({ type: 'bitmap', fileHash, quality, bitmap }, [bitmap]);
    } catch (error) {
      inFlight.delete(fileHash);
      if ((error as Error)?.name === 'AbortError') return;
      failCounts.set(fileHash, (failCounts.get(fileHash) ?? 0) + 1);
      ctx.postMessage({ type: 'error', fileHash, quality });
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
