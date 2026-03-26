/**
 * Frontend API layer — the only place that knows backend command names.
 * Controllers and features call these methods, never raw invoke().
 */

import { invoke } from './ipc';
import type {
  SidebarTreeResponse, EntityViewQuery, EntityViewPage,
  CanonicalEntityGridItem,
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

export function renameFolder(folderId: number, name: string): Promise<void> {
  return invoke<void>('update_folder', { folder_id: folderId, name });
}

export function updateFolder(folderId: number, patch: {
  name?: string;
  icon?: string | null;
  color?: string | null;
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

// ── Smart folders ────────────────────────────────────────────────

export function deleteSmartFolder(id: string): Promise<void> {
  return invoke<void>('delete_smart_folder', { id });
}

// NOTE: update_smart_folder requires a full SmartFolder struct.
// No partial patch command exists. Rename/color/icon for smart folders
// are blocked until the backend adds partial update support.

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
