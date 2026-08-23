/// <reference lib="webworker" />

/**
 * Thumbnail decode worker — owns loading, cancellation, and decoding.
 *
 * The main thread sends a "plan" (visible hashes + URLs) each frame.
 * Decoded bitmaps are transferred immediately. Reveal timing belongs to the
 * main-thread ThumbnailRevealTracker.
 */

// ── Messages ────────────────────────────────────────────────────

type PlanEntry = { hash: string; url: string };
type PlanMessage = { type: 'plan'; entries: PlanEntry[] };
type ClearMessage = { type: 'clear' };
type IncomingMessage = PlanMessage | ClearMessage;

// ── State ───────────────────────────────────────────────────────

const ctx = self as DedicatedWorkerGlobalScope;

const currentPlan = new Map<string, string>();          // hash → url
const inFlight = new Map<string, AbortController>();
const delivered = new Map<string, string>();             // hash -> transferred URL
const failCounts = new Map<string, number>();

const MAX_CONCURRENT = 6;
const MAX_FAILURES = 2;

// ── Plan ────────────────────────────────────────────────────────

function handlePlan(entries: PlanEntry[]): void {
  // Build next plan as a set for O(1) lookups
  const nextHashes = new Set<string>();
  const nextUrls = new Map<string, string>();
  for (let i = 0; i < entries.length; i++) {
    nextHashes.add(entries[i].hash);
    nextUrls.set(entries[i].hash, entries[i].url);
  }

  // Cancel loads no longer in plan
  for (const [hash, controller] of inFlight) {
    if (!nextHashes.has(hash)) {
      controller.abort();
      inFlight.delete(hash);
    }
  }

  for (const [hash, url] of delivered) {
    if (!nextHashes.has(hash) || nextUrls.get(hash) !== url) delivered.delete(hash);
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

// ── Loading ─────────────────────────────────────────────────────

function pumpLoads(): void {
  for (const [hash, url] of currentPlan) {
    if (inFlight.size >= MAX_CONCURRENT) break;
    if (inFlight.has(hash) || delivered.has(hash)) continue;
    if ((failCounts.get(hash) ?? 0) >= MAX_FAILURES) continue;
    startLoad(hash, url);
  }
}

function startLoad(hash: string, url: string): void {
  const controller = new AbortController();
  inFlight.set(hash, controller);

  void (async () => {
    try {
      const response = await fetch(url, { signal: controller.signal });
      if (!response.ok) throw new Error(`fetch ${response.status}`);
      const blob = await response.blob();
      const bitmap = await createImageBitmap(blob);

      if (controller.signal.aborted) { bitmap.close(); return; }
      inFlight.delete(hash);

      if (!currentPlan.has(hash)) { bitmap.close(); return; }

      delivered.set(hash, url);
      ctx.postMessage({ type: 'bitmap', hash, bitmap }, [bitmap]);
    } catch (error) {
      inFlight.delete(hash);
      if ((error as Error)?.name === 'AbortError') return;
      failCounts.set(hash, (failCounts.get(hash) ?? 0) + 1);
      ctx.postMessage({ type: 'error', hash });
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
};
