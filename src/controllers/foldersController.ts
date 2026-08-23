/**
 * Folder controller — owns folder CRUD actions.
 * Calls API and settles confirmed sidebar updates.
 */

import { getDefaultStore } from 'jotai';
import {
  clearFolderWatchConfig,
  createFolder,
  deleteFolder,
  getFolderCoverHashes,
  addMedia,
  moveFolder,
  renameFolder,
  reorderFolderItems,
  setFolderWatchConfig,
  updateFolder,
} from '../platform/folderApi';
import type { SidebarNodeDto } from '../shared/types/canonical';
import { activeNodeIdAtom } from '../state/navigation';
import { removeHistoryEntries, pushHistory } from '../state/navigationHistory';
import { patchFolderNodeAtom, removeFolderNodesAtom, sidebarNodesAtom } from '../state/sidebar';

const store = getDefaultStore();

interface FolderDeletionPlan {
  rootFolderIds: number[];
  deletedNodeIds: Set<string>;
  parentByNodeId: Map<string, string | null>;
}

function folderNodeId(folderId: number): string {
  return `folder:${folderId}`;
}

/**
 * Expand selected roots using the current sidebar tree. A selected descendant
 * is omitted when its ancestor is already selected, so the backend sees one
 * delete command per independent hierarchy.
 */
export function planFolderDeletion(nodes: SidebarNodeDto[], folderIds: number[]): FolderDeletionPlan {
  const folders = nodes.filter((node) => node.kind === 'folder');
  const byId = new Map(folders.map((node) => [node.id, node]));
  const selected = new Set(folderIds.map(folderNodeId).filter((nodeId) => byId.has(nodeId)));
  const roots = [...selected].filter((nodeId) => {
    let parentId = byId.get(nodeId)?.parent_id ?? null;
    while (parentId?.startsWith('folder:')) {
      if (selected.has(parentId)) return false;
      parentId = byId.get(parentId)?.parent_id ?? null;
    }
    return true;
  });
  const deletedNodeIds = new Set(roots);

  let added = true;
  while (added) {
    added = false;
    for (const folder of folders) {
      if (folder.parent_id && deletedNodeIds.has(folder.parent_id) && !deletedNodeIds.has(folder.id)) {
        deletedNodeIds.add(folder.id);
        added = true;
      }
    }
  }

  return {
    rootFolderIds: roots.map((nodeId) => Number(nodeId.slice('folder:'.length))),
    deletedNodeIds,
    parentByNodeId: new Map(folders.map((node) => [node.id, node.parent_id])),
  };
}

function nearestSurvivingParent(plan: FolderDeletionPlan, nodeId: string): string {
  let parentId = plan.parentByNodeId.get(nodeId) ?? null;
  while (parentId?.startsWith('folder:')) {
    if (!plan.deletedNodeIds.has(parentId)) return parentId;
    parentId = plan.parentByNodeId.get(parentId) ?? null;
  }
  return 'system:active';
}

/** Apply a backend-confirmed recursive folder deletion to every dependent UI state. */
export function settleFolderDeletion(plan: FolderDeletionPlan) {
  if (plan.deletedNodeIds.size === 0) return;

  const activeNodeId = store.get(activeNodeIdAtom);
  const fallbackNodeId = plan.deletedNodeIds.has(activeNodeId)
    ? nearestSurvivingParent(plan, activeNodeId)
    : null;

  store.set(removeFolderNodesAtom, plan.deletedNodeIds);
  removeHistoryEntries(plan.deletedNodeIds);

  if (fallbackNodeId) {
    store.set(activeNodeIdAtom, fallbackNodeId);
    pushHistory(fallbackNodeId);
  }
}

/** Settle a delete_folder state change, including descendants omitted by the backend event. */
export function settleFolderDeletionFromSidebar(folderIds: number[]) {
  settleFolderDeletion(planFolderDeletion(store.get(sidebarNodesAtom), folderIds));
}

export function singleFolderDeletionMessage(name: string): string {
  return `Delete "${name}" and all its subfolders? Media inside these folders will remain untouched.`;
}

export function bulkFolderDeletionMessage(selectedCount: number): string {
  return `Delete ${selectedCount} selected item${selectedCount === 1 ? '' : 's'}? All selected folders and their subfolders will be deleted. Media inside these folders will remain untouched.`;
}

export const foldersController = {
  async create(name: string, parentId?: number | null): Promise<string | null> {
    const result = await createFolder({ name, parent_id: parentId ?? null });
    // Return the node ID if the backend returned a folder_id
    const folderId = result && typeof result === 'object' && 'folder_id' in result
      ? (result as { folder_id: number }).folder_id
      : null;
    return folderId != null ? `folder:${folderId}` : null;
  },

  async rename(folderId: number, newName: string) {
    store.set(patchFolderNodeAtom, { folderId, patch: { name: newName } });
    await renameFolder(folderId, newName);
  },

  async delete(folderId: number) {
    await this.deleteMany([folderId]);
  },

  async deleteMany(folderIds: number[]) {
    const plan = planFolderDeletion(store.get(sidebarNodesAtom), folderIds);
    if (plan.rootFolderIds.length === 0) return;
    await Promise.all(plan.rootFolderIds.map((folderId) => deleteFolder(folderId)));
    settleFolderDeletion(plan);
  },

  async applyColor(folderId: number, color: string | null) {
    store.set(patchFolderNodeAtom, { folderId, patch: { color } });
    await updateFolder(folderId, { color });
  },

  async applyIcon(folderId: number, icon: string | null) {
    store.set(patchFolderNodeAtom, { folderId, patch: { icon } });
    await updateFolder(folderId, { icon });
  },

  async applyNotes(folderId: number, notes: string | null) {
    await updateFolder(folderId, { notes });
  },

  async move(folderId: number, parentFolderId: number | null, moves: [number, number][]) {
    await moveFolder(folderId, parentFolderId, moves);
  },

  async sortByName(folderId: number) {
    await reorderFolderItems(folderId, { sort_by: 'name', direction: 'asc' });
  },

  async addMedia(folderPath: string, parentFolderId: number | null) {
    await addMedia([folderPath], {
      parent_folder_id: parentFolderId,
      preserve_structure: true,
    });
  },

  getCoverHashes(folderIds: number[]) {
    return getFolderCoverHashes(folderIds);
  },

  async setWatchConfig(folderId: number, config: {
    watchPath: string;
    enabled: boolean;
    subfolders: boolean;
    importStatusMode: string;
  }) {
    await setFolderWatchConfig(folderId, {
      watch_path: config.watchPath,
      watch_enabled: config.enabled,
      watch_subfolders: config.subfolders,
      watch_import_status_mode: config.importStatusMode,
    });
  },

  async clearWatchConfig(folderId: number) {
    await clearFolderWatchConfig(folderId);
  },
};
