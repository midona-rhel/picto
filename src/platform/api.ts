/**
 * Frontend API layer — the only place that knows backend command names.
 * Controllers and features call these methods, never raw invoke().
 */

import { invoke } from './ipc';
import type { SidebarTreeResponse } from '../shared/types/canonical';

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
