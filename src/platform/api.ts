/**
 * Frontend API layer — the only place that knows backend command names.
 * Controllers and features call these methods, never raw invoke().
 */

import { invoke } from './ipc';
import type {
  SidebarTreeResponse, EntityViewQuery, EntityViewPage,
  CanonicalEntityGridItem, CanonicalEntityDetails,
  EntityTarget, MediaEntityPatch, CanonicalTagRecord,
  CanonicalTagRelation, CanonicalNamespaceSummary, SelectionSummary,
} from '../shared/types/canonical';

// ── Grid ────────────────────────────────────────────────────────

/**
 * Query the grid via canonical query_entity_view command.
 * Both loading and reconcile now use the same backend path (ApplicationEngine).
 */
export function queryEntityView(query: EntityViewQuery): Promise<EntityViewPage> {
  return invoke<EntityViewPage>('query_entity_view', query as unknown as Record<string, unknown>);
}

export interface ReconcileResult {
  kind: 'no_change' | 'patch_rows' | 'replace_window' | 'full_refresh_required';
  items?: CanonicalEntityGridItem[];
  page?: EntityViewPage;
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

// ── Inspector / entity details ───────────────────────────────────

export function getEntityDetails(entityHash: string): Promise<CanonicalEntityDetails | null> {
  return invoke<CanonicalEntityDetails | null>('get_entity_details', { entity_hash: entityHash });
}

// ── Entity mutations ─────────────────────────────────────────────

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

export function getSelectionSummary(target: EntityTarget): Promise<SelectionSummary> {
  return invoke<SelectionSummary>('get_selection_summary', { target } as unknown as Record<string, unknown>);
}

export function searchTags(query: string, limit = 20, offset = 0): Promise<CanonicalTagRecord[]> {
  return invoke<CanonicalTagRecord[]>('search_tags', { query, limit, offset });
}

export function getTagsPaginated(params: {
  namespace?: string | null;
  search?: string | null;
  cursor?: string | null;
  limit?: number;
}): Promise<CanonicalTagRecord[]> {
  return invoke<CanonicalTagRecord[]>('get_tags_paginated', params as unknown as Record<string, unknown>);
}

export function getNamespaceSummary(): Promise<CanonicalNamespaceSummary[]> {
  return invoke<CanonicalNamespaceSummary[]>('get_namespace_summary');
}

export function getTagRelations(tagId: number, relationType: 'aliases' | 'implications'): Promise<CanonicalTagRelation[]> {
  return invoke<CanonicalTagRelation[]>('get_tag_relations', {
    tag_id: tagId,
    relation_type: relationType,
  });
}

export function renameTag(tagId: number, newName: string): Promise<unknown> {
  return invoke('rename_tag', { tag_id: tagId, new_name: newName });
}

export function mergeTags(fromTag: string, toTag: string): Promise<unknown> {
  return invoke('merge_tags', { from_tag: fromTag, to_tag: toTag });
}

export function deleteTag(tagId: number): Promise<unknown> {
  return invoke('delete_tag', { tag_id: tagId });
}

export function manageTagAlias(from: string, to?: string | null): Promise<void> {
  return invoke<void>('manage_tag_alias', { from, to: to ?? null });
}

export function manageTagImplication(
  child: string,
  parent: string,
  action: 'add' | 'remove',
): Promise<void> {
  return invoke<void>('manage_tag_implication', { child, parent, action });
}

export function setTagSiteMask(tagId: number, siteMask: string): Promise<void> {
  return invoke<void>('set_tag_site_mask', { tag_id: tagId, site_mask: siteMask });
}

// ── Sidebar ──────────────────────────────────────────────────────

export function getSidebarTree(): Promise<SidebarTreeResponse> {
  return invoke<SidebarTreeResponse>('get_sidebar_tree');
}

export function reorderSidebarNodes(moves: [string, number][]): Promise<void> {
  return invoke<void>('reorder_sidebar_nodes', { moves });
}

// ── Folders ──────────────────────────────────────────────────────

export function createFolder(params: {
  name: string;
  parent_id?: number | null;
  icon?: string;
  color?: string;
}): Promise<unknown> {
  return invoke('create_folder', params);
}

export function deleteFolder(folderId: number): Promise<void> {
  return invoke<void>('delete_folder', { folder_id: folderId });
}

export function removeEntitiesFromFolder(folderId: number, hashes: string[]): Promise<void> {
  return invoke<void>('remove_entities_from_folder', { folder_id: folderId, hashes });
}

export function renameFolder(folderId: number, name: string): Promise<void> {
  return invoke<void>('update_folder', { folder_id: folderId, name });
}

export function updateFolder(folderId: number, patch: {
  name?: string;
  icon?: string | null;
  color?: string | null;
  notes?: string | null;
}): Promise<void> {
  return invoke<void>('update_folder', { folder_id: folderId, ...patch });
}

export function moveFolder(
  folderId: number,
  newParentId: number | null,
  siblingOrder: [number, number][],
): Promise<void> {
  return invoke<void>('move_folder', {
    folder_id: folderId,
    new_parent_id: newParentId,
    sibling_order: siblingOrder,
  });
}

export function updateFolderMembership(
  target: EntityTarget,
  folderId: number,
  operation: 'add' | 'remove',
): Promise<unknown> {
  return invoke('update_folder_membership', {
    target,
    folder_id: folderId,
    operation,
  } as unknown as Record<string, unknown>);
}

// ── Smart folders ────────────────────────────────────────────────

export function deleteSmartFolder(id: string): Promise<void> {
  return invoke<void>('delete_smart_folder', { id });
}

// NOTE: update_smart_folder requires a full SmartFolder struct.
// No partial patch command exists.

export function updateSmartFolder(params: {
  id: string;
  folder: {
    smart_folder_id: number;
    name: string;
    parent_id: number | null;
    icon: string | null;
    color: string | null;
    notes: string | null;
    predicate_json: string;
    sort_field: string | null;
    sort_order: string | null;
    display_order: number | null;
    created_at: string | null;
    updated_at: string | null;
  };
}): Promise<void> {
  return invoke<void>('update_smart_folder', params);
}

export function moveSmartFolder(
  smartFolderId: number,
  newParentId: number | null,
  siblingOrder: [number, number][],
): Promise<void> {
  return invoke<void>('move_smart_folder', {
    smart_folder_id: smartFolderId,
    new_parent_id: newParentId,
    sibling_order: siblingOrder,
  });
}
