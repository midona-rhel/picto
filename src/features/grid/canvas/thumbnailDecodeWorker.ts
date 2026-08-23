/// <reference lib="webworker" />

/**
 * Thumbnail decode worker — owns loading, caching, and reveal staggering.
 *
 * The main thread sends a "plan" (visible physical file hashes + URLs) each frame.
 * The worker fetches, decodes, and releases bitmaps one at a time
 * every STAGGER_MS so the main thread gets a smooth reveal cascade.
 */

// ── Messages ────────────────────────────────────────────────────

type PlanEntry = { fileHash: string; url: string };
type PlanMessage = { type: 'plan'; entries: PlanEntry[] };
type ClearMessage = { type: 'clear' };
type IncomingMessage = PlanMessage | ClearMessage;

// ── State ───────────────────────────────────────────────────────

const ctx = self as DedicatedWorkerGlobalScope;

const currentPlan = new Map<string, string>();          // file hash → url
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
  const nextFileHashes = new Set<string>();
  const nextUrls = new Map<string, string>();
  for (let i = 0; i < entries.length; i++) {
    nextFileHashes.add(entries[i].fileHash);
    nextUrls.set(entries[i].fileHash, entries[i].url);
  }

  // Cancel loads no longer in plan
  for (const [fileHash, controller] of inFlight) {
    if (!nextFileHashes.has(fileHash)) {
      controller.abort();
      inFlight.delete(fileHash);
    }
  }

  // Drop decoded bitmaps no longer in plan
  for (const [fileHash, bitmap] of decoded) {
    if (!nextFileHashes.has(fileHash)) {
      bitmap.close();
      decoded.delete(fileHash);
    }
  }

  // Prune reveal queue — filter instead of splice-in-loop
  if (revealQueue.length > 0) {
    revealQueue = revealQueue.filter(fileHash => nextFileHashes.has(fileHash));
  }

  // Forget revealed hashes that left the plan (so they re-decode on re-entry)
  // Also re-fetch if the URL changed (quality upgrade/downgrade)
  for (const fileHash of revealed) {
    if (!nextFileHashes.has(fileHash)) { revealed.delete(fileHash); continue; }
    const oldUrl = currentPlan.get(fileHash);
    const newUrl = nextUrls.get(fileHash);
    if (oldUrl && newUrl && oldUrl !== newUrl) revealed.delete(fileHash);
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
  for (const [fileHash, url] of currentPlan) {
    if (inFlight.size >= MAX_CONCURRENT) break;
    if (inFlight.has(fileHash) || decoded.has(fileHash) || revealed.has(fileHash)) continue;
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

      decoded.set(fileHash, bitmap);
      revealQueue.push(fileHash);
      drainRevealQueue();
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

// ── Reveal stagger ──────────────────────────────────────────────

function drainRevealQueue(): void {
  if (drainTimer || revealQueue.length === 0) return;

  const fileHash = revealQueue.shift()!;
  const bitmap = decoded.get(fileHash);
  decoded.delete(fileHash);

  if (!bitmap || !currentPlan.has(fileHash)) {
    bitmap?.close();
    drainRevealQueue();
    return;
  }

  revealed.add(fileHash);
  ctx.postMessage({ type: 'reveal', fileHash, bitmap }, [bitmap]);

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
