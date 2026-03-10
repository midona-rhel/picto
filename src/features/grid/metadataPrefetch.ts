/**
 * Thin metadata pass-through — dedupes in-flight requests only.
 * No LRU cache. Backend (SQLite) is the single source of truth.
 */
import { api } from '#desktop/api';
import type {
  EntityAllMetadata,
  ResolvedTagInfo,
  SelectionQuerySpec,
  SelectionSummary,
} from '../../shared/types/api';

export type { EntityAllMetadata, ResolvedTagInfo, SelectionQuerySpec, SelectionSummary };

// ---------------------------------------------------------------------------
// In-flight dedup only — no result caching
// ---------------------------------------------------------------------------

const inflight = new Map<string, Promise<EntityAllMetadata>>();

function fetchSingle(hash: string): Promise<EntityAllMetadata> {
  const existing = inflight.get(hash);
  if (existing) return existing;
  const p = api.file.getAllMetadata(hash).finally(() => inflight.delete(hash));
  inflight.set(hash, p);
  return p;
}

/** Fire-and-forget prefetch for a single hash. */
export function prefetchMetadata(hash: string): void {
  void fetchSingle(hash);
}

/** Batch prefetch — hits the projection-backed batch endpoint. */
export async function prefetchMetadataBatch(hashes: string[]): Promise<void> {
  const unique = [...new Set(hashes)].filter(Boolean);
  if (unique.length === 0) return;
  const MAX_BATCH = 200;
  for (let i = 0; i < unique.length; i += MAX_BATCH) {
    await api.grid.getFilesMetadataBatch(unique.slice(i, i + MAX_BATCH));
  }
}

/** Get metadata for a hash — returns a fresh fetch every time. */
export function getMetadata(hash: string): Promise<EntityAllMetadata> {
  return fetchSingle(hash);
}

/** No-op — no cache to invalidate. */
export function invalidateMetadata(_hash: string): void {}
export function invalidateManyMetadata(_hashes: string[]): void {}

/** No-op — no cache to pin. */
export function pinMetadata(_hash: string): void {}
export function unpinMetadata(_hash: string): void {}

// ---------------------------------------------------------------------------
// Selection summary — simple in-flight dedup
// ---------------------------------------------------------------------------

const selectionInflight = new Map<string, Promise<SelectionSummary>>();

function stableKey(spec: SelectionQuerySpec): string {
  return JSON.stringify(spec);
}

export function getOrStartSelectionSummary(spec: SelectionQuerySpec): Promise<SelectionSummary> {
  const key = stableKey(spec);
  const existing = selectionInflight.get(key);
  if (existing) return existing;
  const p = api.selection.getSummary(spec).finally(() => selectionInflight.delete(key));
  selectionInflight.set(key, p);
  return p;
}

export function invalidateSelectionSummary(_selectionKey?: string): void {
  selectionInflight.clear();
}

export function getMetadataCacheDebugStats() {
  return { entries: inflight.size, bytes: 0, budgetBytes: 0 };
}
