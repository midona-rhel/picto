/**
 * Shared metadata + selection-summary cache for grid browsing.
 *
 * Goals:
 * - dedupe in-flight requests
 * - batch prefetch visible items
 * - keep memory bounded (byte-budgeted LRU)
 * - keep selected/visible items pinned
 */
import { api } from '#desktop/api';
import type {
  EntityAllMetadata,
  ResolvedTagInfo,
  SelectionQuerySpec,
  SelectionSummary,
} from '../../shared/types/api';

export type { EntityAllMetadata };
export type {
  ResolvedTagInfo,
  SelectionQuerySpec,
  SelectionSummary,
};

type MetadataEntry = {
  promise: Promise<EntityAllMetadata>;
  value?: EntityAllMetadata;
  approxBytes: number;
  lastTouch: number;
  pinCount: number;
};

const METADATA_CACHE_BUDGET_BYTES = 2 * 1024 * 1024 * 1024; // user-requested target
const metadataCache = new Map<string, MetadataEntry>();
let metadataCacheBytes = 0;

const selectionSummaryCache = new Map<string, Promise<SelectionSummary>>();
let metadataBatchPrefetchChain: Promise<void> = Promise.resolve();

type Deferred<T> = {
  promise: Promise<T>;
  resolve: (value: T | PromiseLike<T>) => void;
  reject: (reason?: unknown) => void;
};

function createDeferred<T>(): Deferred<T> {
  let resolve!: (value: T | PromiseLike<T>) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function nowTs(): number {
  return performance.now();
}

function stableSelectionKey(spec: SelectionQuerySpec): string {
  const normalized = {
    ...spec,
    hashes: spec.hashes ? [...spec.hashes].sort() : null,
    search_tags: spec.search_tags ? [...spec.search_tags].sort() : null,
    search_excluded_tags: spec.search_excluded_tags ? [...spec.search_excluded_tags].sort() : null,
    excluded_hashes: spec.excluded_hashes ? [...spec.excluded_hashes].sort() : null,
    included_hashes: spec.included_hashes ? [...spec.included_hashes].sort() : null,
    folder_ids: spec.folder_ids ? [...spec.folder_ids].sort((a, b) => a - b) : null,
    excluded_folder_ids: spec.excluded_folder_ids ? [...spec.excluded_folder_ids].sort((a, b) => a - b) : null,
  };
  return JSON.stringify(normalized);
}

function estimateMetadataBytes(metadata: EntityAllMetadata): number {
  // Raw serialized bytes + safety factor for object/string overhead in JS heap.
  try {
    return Math.max(512, Math.ceil(JSON.stringify(metadata).length * 2.25));
  } catch {
    return 4096;
  }
}

function touchEntry(hash: string): void {
  const entry = metadataCache.get(hash);
  if (!entry) return;
  entry.lastTouch = nowTs();
}

function evictMetadataCache(): void {
  if (metadataCacheBytes <= METADATA_CACHE_BUDGET_BYTES) return;

  const evictable = [...metadataCache.entries()]
    .filter(([, e]) => e.pinCount <= 0)
    .sort((a, b) => a[1].lastTouch - b[1].lastTouch);

  for (const [hash, entry] of evictable) {
    if (metadataCacheBytes <= METADATA_CACHE_BUDGET_BYTES) break;
    metadataCache.delete(hash);
    metadataCacheBytes = Math.max(0, metadataCacheBytes - (entry.approxBytes || 0));
  }
}

function upsertResolvedMetadata(hash: string, metadata: EntityAllMetadata): Promise<EntityAllMetadata> {
  const existing = metadataCache.get(hash);
  const approxBytes = estimateMetadataBytes(metadata);
  const promise = Promise.resolve(metadata);

  if (existing) {
    metadataCacheBytes = Math.max(0, metadataCacheBytes - (existing.approxBytes || 0));
    existing.promise = promise;
    existing.value = metadata;
    existing.approxBytes = approxBytes;
    existing.lastTouch = nowTs();
    metadataCacheBytes += approxBytes;
    evictMetadataCache();
    return existing.promise;
  }

  metadataCache.set(hash, {
    promise,
    value: metadata,
    approxBytes,
    lastTouch: nowTs(),
    pinCount: 0,
  });
  metadataCacheBytes += approxBytes;
  evictMetadataCache();
  return promise;
}

function startSingleMetadataFetch(hash: string): Promise<EntityAllMetadata> {
  const startedAt = nowTs();
  const promise = api.file.getAllMetadata(hash).then((data) => {
    if (import.meta.env.DEV) console.log(`[props-perf] metadata arrived for ${hash.slice(0, 8)} in ${(nowTs() - startedAt).toFixed(1)}ms`);
    upsertResolvedMetadata(hash, data);
    return data;
  }).catch((err) => {
    metadataCache.delete(hash);
    throw err;
  });

  metadataCache.set(hash, {
    promise,
    approxBytes: 0,
    lastTouch: nowTs(),
    pinCount: metadataCache.get(hash)?.pinCount ?? 0,
  });
  return promise;
}

/** Start fetching metadata immediately. Safe to call multiple times for the same hash. */
export function prefetchMetadata(hash: string): void {
  if (metadataCache.has(hash)) {
    touchEntry(hash);
    return;
  }
  if (import.meta.env.DEV) console.log(`[props-perf] click → prefetch started for ${hash.slice(0, 8)}`);
  void startSingleMetadataFetch(hash);
}

async function prefetchMetadataBatchInternal(hashes: string[]): Promise<void> {
  const unique = [...new Set(hashes)].filter(Boolean);
  const missing = unique.filter((h) => !metadataCache.has(h));
  if (missing.length === 0) {
    unique.forEach(touchEntry);
    return;
  }

  const chunks: string[][] = [];
  const MAX_BATCH = 200;
  for (let i = 0; i < missing.length; i += MAX_BATCH) {
    chunks.push(missing.slice(i, i + MAX_BATCH));
  }

  for (const chunk of chunks) {
    const deferredByHash = new Map<string, Deferred<EntityAllMetadata>>();
    const chunkToFetch: string[] = [];

    // Insert in-flight placeholders so overlapping prefetch calls do not duplicate backend work.
    for (const hash of chunk) {
      if (metadataCache.has(hash)) {
        touchEntry(hash);
        continue;
      }
      const deferred = createDeferred<EntityAllMetadata>();
      deferredByHash.set(hash, deferred);
      metadataCache.set(hash, {
        promise: deferred.promise,
        approxBytes: 0,
        lastTouch: nowTs(),
        pinCount: 0,
      });
      chunkToFetch.push(hash);
    }

    if (chunkToFetch.length === 0) continue;

    try {
      const resp = await api.grid.getFilesMetadataBatch(chunkToFetch);
      for (const [hash, metadata] of Object.entries(resp.items ?? {})) {
        upsertResolvedMetadata(hash, metadata);
        deferredByHash.get(hash)?.resolve(metadata);
        deferredByHash.delete(hash);
      }

      // Any hash not returned by the backend is removed from the cache and rejected.
      for (const [hash, deferred] of deferredByHash.entries()) {
        metadataCache.delete(hash);
        deferred.reject(new Error(`Metadata not found for hash ${hash}`));
      }
    } catch (err) {
      // Fall back to per-item prefetch for resilience.
      console.warn('Batch metadata prefetch failed, falling back to singles:', err);
      for (const [hash, deferred] of deferredByHash.entries()) {
        metadataCache.delete(hash);
        deferred.reject(err);
      }
      chunkToFetch.forEach(prefetchMetadata);
    }
  }
}

/** Batch prefetch metadata for many hashes using projection-backed backend command. */
export function prefetchMetadataBatch(hashes: string[]): Promise<void> {
  const requestHashes = [...hashes];
  metadataBatchPrefetchChain = metadataBatchPrefetchChain
    .catch(() => {
      // Keep the chain alive after prior failures.
    })
    .then(() => prefetchMetadataBatchInternal(requestHashes));
  return metadataBatchPrefetchChain;
}

/**
 * Return the in-flight or completed promise. Shared — multiple consumers can
 * call this for the same hash. Starts a new fetch if nothing was prefetched.
 */
export function getMetadata(hash: string): Promise<EntityAllMetadata> {
  const existing = metadataCache.get(hash);
  if (existing) {
    touchEntry(hash);
    return existing.promise;
  }
  return startSingleMetadataFetch(hash);
}

/** Invalidate cached metadata (e.g. after editing tags). */
export function invalidateMetadata(hash: string): void {
  const entry = metadataCache.get(hash);
  if (entry) {
    metadataCacheBytes = Math.max(0, metadataCacheBytes - (entry.approxBytes || 0));
  }
  metadataCache.delete(hash);
}

export function invalidateManyMetadata(hashes: string[]): void {
  for (const h of hashes) invalidateMetadata(h);
}

export function pinMetadata(hash: string): void {
  const entry = metadataCache.get(hash);
  if (!entry) return;
  entry.pinCount += 1;
  touchEntry(hash);
}

export function unpinMetadata(hash: string): void {
  const entry = metadataCache.get(hash);
  if (!entry) return;
  entry.pinCount = Math.max(0, entry.pinCount - 1);
  touchEntry(hash);
  evictMetadataCache();
}

export function getOrStartSelectionSummary(spec: SelectionQuerySpec): Promise<SelectionSummary> {
  const key = stableSelectionKey(spec);
  const existing = selectionSummaryCache.get(key);
  if (existing) return existing;
  const promise = api.selection.getSummary(spec).catch((err) => {
    selectionSummaryCache.delete(key);
    throw err;
  });
  selectionSummaryCache.set(key, promise);
  return promise;
}

export function invalidateSelectionSummary(selectionKey?: string): void {
  if (selectionKey) selectionSummaryCache.delete(selectionKey);
  else selectionSummaryCache.clear();
}

export function getMetadataCacheDebugStats() {
  return {
    entries: metadataCache.size,
    bytes: metadataCacheBytes,
    budgetBytes: METADATA_CACHE_BUDGET_BYTES,
  };
}
