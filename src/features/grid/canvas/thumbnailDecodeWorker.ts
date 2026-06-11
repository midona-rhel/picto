/// <reference lib="webworker" />

/**
 * Thumbnail decode worker — owns loading, caching, and reveal staggering.
 *
 * The main thread sends a "plan" (visible hashes + URLs) each frame.
 * The worker fetches, decodes, and releases bitmaps one at a time
 * every STAGGER_MS so the main thread gets a smooth reveal cascade.
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
const decoded = new Map<string, ImageBitmap>();          // decoded, waiting to reveal
const revealed = new Set<string>();                      // already transferred to main
const failCounts = new Map<string, number>();
let revealQueue: string[] = [];
let drainTimer: ReturnType<typeof setTimeout> | null = null;

const MAX_CONCURRENT = 6;
const STAGGER_MS = 16;
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

  // Drop decoded bitmaps no longer in plan
  for (const [hash, bitmap] of decoded) {
    if (!nextHashes.has(hash)) {
      bitmap.close();
      decoded.delete(hash);
    }
  }

  // Prune reveal queue — filter instead of splice-in-loop
  if (revealQueue.length > 0) {
    revealQueue = revealQueue.filter(h => nextHashes.has(h));
  }

  // Forget revealed hashes that left the plan (so they re-decode on re-entry)
  // Also re-fetch if the URL changed (quality upgrade/downgrade)
  for (const hash of revealed) {
    if (!nextHashes.has(hash)) { revealed.delete(hash); continue; }
    const oldUrl = currentPlan.get(hash);
    const newUrl = nextUrls.get(hash);
    if (oldUrl && newUrl && oldUrl !== newUrl) revealed.delete(hash);
  }

  currentPlan.clear();
  for (const [h, u] of nextUrls) currentPlan.set(h, u);

  pumpLoads();
}

function handleClear(): void {
  for (const c of inFlight.values()) c.abort();
  inFlight.clear();
  for (const b of decoded.values()) b.close();
  decoded.clear();
  revealed.clear();
  failCounts.clear();
  revealQueue.length = 0;
  currentPlan.clear();
  if (drainTimer) { clearTimeout(drainTimer); drainTimer = null; }
}

// ── Loading ─────────────────────────────────────────────────────

function pumpLoads(): void {
  for (const [hash, url] of currentPlan) {
    if (inFlight.size >= MAX_CONCURRENT) break;
    if (inFlight.has(hash) || decoded.has(hash) || revealed.has(hash)) continue;
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

      decoded.set(hash, bitmap);
      revealQueue.push(hash);
      drainRevealQueue();
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

// ── Reveal stagger ──────────────────────────────────────────────

function drainRevealQueue(): void {
  if (drainTimer || revealQueue.length === 0) return;

  const hash = revealQueue.shift()!;
  const bitmap = decoded.get(hash);
  decoded.delete(hash);

  if (!bitmap || !currentPlan.has(hash)) {
    bitmap?.close();
    drainRevealQueue();
    return;
  }

  revealed.add(hash);
  ctx.postMessage({ type: 'reveal', hash, bitmap }, [bitmap]);

  if (revealQueue.length > 0) {
    drainTimer = setTimeout(() => {
      drainTimer = null;
      drainRevealQueue();
    }, STAGGER_MS);
  }
}

// ── Entry ───────────────────────────────────────────────────────

ctx.onmessage = (event: MessageEvent<IncomingMessage>) => {
  const msg = event.data;
  if (msg.type === 'plan') handlePlan(msg.entries);
  else if (msg.type === 'clear') handleClear();
};
