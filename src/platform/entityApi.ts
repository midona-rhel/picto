import { invoke } from './ipc';
import type {
  CanonicalEntityDetails,
  CanonicalEntityGridItem,
  EntityTarget,
  EntityViewPage,
  EntityViewQuery,
  MediaEntityPatch,
  SelectionSummary,
} from '../shared/types/canonical';

export interface ReconcileResult {
  kind: 'no_change' | 'patch_rows' | 'replace_window' | 'full_refresh_required';
  items?: CanonicalEntityGridItem[];
  page?: EntityViewPage;
}

export function queryEntityView(query: EntityViewQuery): Promise<EntityViewPage> {
  return invoke<EntityViewPage>('query_entity_view', query as unknown as Record<string, unknown>);
}

export function reconcileEntityView(
  query: EntityViewQuery,
  visibleHashes: string[],
  metadataOnly: boolean,
): Promise<ReconcileResult> {
  return invoke<ReconcileResult>('reconcile_entity_view', {
    query,
    visible_hashes: visibleHashes,
    metadata_only: metadataOnly,
  } as unknown as Record<string, unknown>);
}

export function getEntityGridItems(hashes: string[]): Promise<CanonicalEntityGridItem[]> {
  return invoke<CanonicalEntityGridItem[]>('get_entity_grid_items', { entity_hashes: hashes });
}

export function getEntityDetails(entityHash: string): Promise<CanonicalEntityDetails | null> {
  return invoke<CanonicalEntityDetails | null>('get_entity_details', { entity_hash: entityHash });
}

export function patchMediaEntities(target: EntityTarget, patch: MediaEntityPatch): Promise<unknown> {
  return invoke('patch_media_entities', { target, patch } as unknown as Record<string, unknown>);
}

export function applyEntityTags(
  target: EntityTarget,
  operation: 'add' | 'remove',
  tags: string[],
  provenanceMask?: string | null,
): Promise<unknown> {
  return invoke('apply_entity_tags', {
    target,
    operation,
    tags,
    provenance_mask: provenanceMask ?? null,
  } as unknown as Record<string, unknown>);
}

export function setEntityStatus(target: EntityTarget, status: number): Promise<unknown> {
  return invoke('set_entity_status', { target, status } as unknown as Record<string, unknown>);
}

export function deleteEntities(target: EntityTarget): Promise<unknown> {
  return invoke('delete_entities', { target } as unknown as Record<string, unknown>);
}

export function getSelectionSummary(target: EntityTarget): Promise<SelectionSummary> {
  return invoke<SelectionSummary>('get_selection_summary', { target } as unknown as Record<string, unknown>);
}

export interface SweepOrphanedBlobsResult {
  deleted_count: number;
  freed_bytes: number;
}

/** Remove blob files that no library record references anymore. */
export function sweepOrphanedBlobs(): Promise<SweepOrphanedBlobsResult> {
  return invoke<SweepOrphanedBlobsResult>('sweep_orphaned_blobs', {});
}
