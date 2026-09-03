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
type PlanEntry = {
  fileHash: string;
  url: string;
  quality: DecodeQuality;
  resizeWidth?: number;
  resizeHeight?: number;
};
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

function requestKey(entry: PlanEntry | undefined): string {
  if (!entry) return '';
  return `${entry.url}|${entry.resizeWidth ?? 0}x${entry.resizeHeight ?? 0}`;
}

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
    if (!nextFileHashes.has(fileHash)
      || requestKey(currentPlan.get(fileHash)) !== requestKey(nextEntries.get(fileHash))) {
      controller.abort();
      inFlight.delete(fileHash);
    }
  }

  for (const [fileHash, key] of delivered) {
    if (!nextFileHashes.has(fileHash) || requestKey(nextEntries.get(fileHash)) !== key) delivered.delete(fileHash);
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
  const { fileHash, url, quality, resizeWidth, resizeHeight } = entry;
  const key = requestKey(entry);
  const controller = new AbortController();
  inFlight.set(fileHash, controller);

  void (async () => {
    let stage: 'fetch' | 'decode' = 'fetch';
    let status: number | undefined;
    let contentType: string | undefined;
    let contentBytes: number | undefined;
    try {
      const response = await fetch(url, { signal: controller.signal });
      status = response.status;
      contentType = response.headers.get('content-type') ?? undefined;
      if (!response.ok) throw new Error(`fetch ${response.status}`);
      stage = 'decode';
      const blob = await response.blob();
      contentBytes = blob.size;
      const bitmap = quality === 'full' && resizeWidth && resizeHeight
        ? await createImageBitmap(blob, {
            resizeWidth,
            resizeHeight,
            resizeQuality: 'high',
          })
        : await createImageBitmap(blob);

      if (controller.signal.aborted) { bitmap.close(); return; }
      inFlight.delete(fileHash);

      if (requestKey(currentPlan.get(fileHash)) !== key) { bitmap.close(); return; }

      delivered.set(fileHash, key);
      ctx.postMessage({ type: 'bitmap', fileHash, quality, bitmap }, [bitmap]);
    } catch (error) {
      inFlight.delete(fileHash);
      if ((error as Error)?.name === 'AbortError') return;
      const attempt = (failCounts.get(fileHash) ?? 0) + 1;
      failCounts.set(fileHash, attempt);
      ctx.postMessage({
        type: 'error',
        fileHash,
        quality,
        failure: {
          url,
          stage,
          message: error instanceof Error ? error.message : String(error),
          attempt,
          terminal: attempt >= MAX_FAILURES,
          status,
          contentType,
          contentBytes,
        },
      });
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
