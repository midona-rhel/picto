/**
 * Thin metadata pass-through — dedupes in-flight requests only.
 * No LRU cache. Backend (SQLite) is the single source of truth.
 */
import { entityController } from '../../controllers/entityController';
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

/** Fire-and-forget prefetch for a single hash. */
export function prefetchMetadata(hash: string): void {
  entityController.prefetchMetadata(hash);
}

/** Batch prefetch — hits the projection-backed batch endpoint. */
export async function prefetchMetadataBatch(hashes: string[]): Promise<void> {
  await entityController.prefetchMetadataBatch(hashes);
}

/** Get metadata for a hash — returns a fresh fetch every time. */
export function getMetadata(hash: string): Promise<EntityAllMetadata> {
  return entityController.getMetadata(hash);
}

/** No-op — backend remains the source of truth. */
export function noteMetadataChanged(hash: string): void {
  entityController.noteMetadataChanged(hash);
}
export function noteManyMetadataChanged(hashes: string[]): void {
  entityController.noteManyMetadataChanged(hashes);
}

export function getOrStartSelectionSummary(spec: SelectionQuerySpec): Promise<SelectionSummary> {
  return entityController.getSelectionSummary(spec);
}

export function noteSelectionSummaryChanged(_selectionKey?: string): void {
  entityController.noteSelectionSummaryChanged();
}

export function getMetadataCacheDebugStats() {
  return { entries: 0, bytes: 0, budgetBytes: 0 };
}
