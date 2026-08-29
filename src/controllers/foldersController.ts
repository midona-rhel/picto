/** Folder CRUD through the replacement transaction and invalidation boundary. */

import { getDefaultStore } from 'jotai';
import {
  addMedia,
  clearFolderWatchConfig,
  createFolder,
  deleteFolders,
  duplicateFolder,
  getFolderCover,
  getFolderAutoTags,
  moveFolder,
  renameFolder,
  reorderFolderChildren,
  setFolderMetadata,
  setFolderAutoTags,
  setFolderCover,
  setFolderWatchConfig,
  sortFolderItems,
  type ContentSortField,
  sortFolderTree,
} from '../platform/folderApi';
import type { FolderMutationReceipt } from '../shared/types/generated/application/FolderMutationReceipt';
import { activeNodeIdAtom } from '../state/navigation';
import { navigateToNode, removeHistoryEntries } from '../state/navigationHistory';
import { patchFolderNodeAtom, pendingSidebarRevealNodeIdAtom, sidebarNodesAtom } from '../state/sidebar';
import { announceUndoableMutation } from '../runtime/historyRuntime';

const store = getDefaultStore();

function folderNodeId(folderId: number): string {
  return `folder:${folderId}`;
}

function settleDeletedFolders(receipts: FolderMutationReceipt[]): void {
  const deletedNodeIds = new Set(
    receipts.flatMap((receipt) => receipt.deleted_folder_ids.map(folderNodeId)),
  );
  if (deletedNodeIds.size === 0) return;

  const activeNodeId = store.get(activeNodeIdAtom);
  const activeReceipt = receipts.find((receipt) =>
    receipt.deleted_folder_ids.some((folderId) => folderNodeId(folderId) === activeNodeId));

  removeHistoryEntries(deletedNodeIds);
  if (activeReceipt) {
    const fallbackNodeId = activeReceipt.fallback_folder_id == null
      ? 'system:active'
      : folderNodeId(activeReceipt.fallback_folder_id);
    navigateToNode(fallbackNodeId);
  }
}

function folderMetadata(folderId: number) {
  const node = store.get(sidebarNodesAtom).find((candidate) => candidate.id === folderNodeId(folderId));
  const meta = (node?.meta ?? {}) as Record<string, unknown>;
  return {
    folder_id: folderId,
    icon: node?.icon ?? null,
    color: node?.color ?? null,
    notes: typeof meta.notes === 'string' ? meta.notes : null,
  };
}

export function singleFolderDeletionMessage(name: string): string {
  return `Delete "${name}" and all its subfolders? Media inside these folders will remain untouched.`;
}

export function bulkFolderDeletionMessage(selectedCount: number): string {
  return `Delete ${selectedCount} selected item${selectedCount === 1 ? '' : 's'}? All selected folders and their subfolders will be deleted. Media inside these folders will remain untouched.`;
}

export const foldersController = {
  async create(name: string, parentId?: number | null): Promise<string> {
    const result = await createFolder({ name, parent_id: parentId ?? null });
    await announceUndoableMutation('folders.create');
    const nodeId = folderNodeId(result.folder_id);
    store.set(pendingSidebarRevealNodeIdAtom, nodeId);
    return nodeId;
  },

  async rename(folderId: number, newName: string): Promise<void> {
    const previousName = store.get(sidebarNodesAtom)
      .find((node) => node.id === folderNodeId(folderId))?.name;
    store.set(patchFolderNodeAtom, { folderId, patch: { name: newName } });
    try {
      await renameFolder(folderId, newName);
      await announceUndoableMutation('folders.rename');
      store.set(patchFolderNodeAtom, { folderId, patch: { name: newName } });
    } catch (error) {
      const currentName = store.get(sidebarNodesAtom)
        .find((node) => node.id === folderNodeId(folderId))?.name;
      if (previousName != null && currentName === newName) {
        store.set(patchFolderNodeAtom, { folderId, patch: { name: previousName } });
      }
      throw error;
    }
  },

  async duplicate(folderId: number): Promise<string> {
    const result = await duplicateFolder(folderId);
    await announceUndoableMutation('folders.duplicate');
    return folderNodeId(result.folder_id);
  },

  async delete(folderId: number): Promise<void> {
    await this.deleteMany([folderId]);
  },

  async deleteMany(folderIds: number[]): Promise<void> {
    const receipt = await deleteFolders([...new Set(folderIds)]);
    settleDeletedFolders([receipt]);
    await announceUndoableMutation('folders.delete');
  },

  async applyColor(folderId: number, color: string | null): Promise<void> {
    await setFolderMetadata({ ...folderMetadata(folderId), color });
    await announceUndoableMutation('folders.set_metadata');
  },

  async applyIcon(folderId: number, icon: string | null): Promise<void> {
    await setFolderMetadata({ ...folderMetadata(folderId), icon });
    await announceUndoableMutation('folders.set_metadata');
  },

  async applyNotes(folderId: number, notes: string | null): Promise<void> {
    await setFolderMetadata({ ...folderMetadata(folderId), notes });
    await announceUndoableMutation('folders.set_metadata');
  },

  getAutoTags(folderId: number): Promise<string[]> {
    return getFolderAutoTags(folderId);
  },

  async setAutoTags(folderId: number, tags: string[]): Promise<void> {
    await setFolderAutoTags(folderId, tags);
    await announceUndoableMutation('folders.set_auto_tags');
  },

  async move(folderId: number, parentFolderId: number | null, moves: [number, number][]) {
    await moveFolder(folderId, parentFolderId);
    if (moves.length > 0) {
      const orderedIds = [...moves]
        .sort((left, right) => left[1] - right[1])
        .map(([siblingId]) => siblingId);
      await reorderFolderChildren(parentFolderId, orderedIds);
    }
    await announceUndoableMutation(moves.length > 0 ? 'folders.reorder' : 'folders.move');
  },

  async sortContents(folderId: number, field: ContentSortField): Promise<void> {
    await sortFolderItems(folderId, field);
    await announceUndoableMutation('folders.sort_items');
  },

  async sortTree(folderId: number, descending: boolean, recursive: boolean): Promise<void> {
    await sortFolderTree(folderId, descending, recursive);
    await announceUndoableMutation('folders.sort_tree');
  },

  async addMedia(folderPath: string, parentFolderId: number | null): Promise<void> {
    await addMedia([folderPath], {
      lifecycle: 'active',
      parent_folder_id: parentFolderId,
      preserve_structure: true,
    });
  },

  getCoverHash(folderId: number): Promise<string | null> {
    return getFolderCover(folderId).then((cover) => cover?.entity_hash ?? null);
  },

  async setCover(folderId: number, itemId: number): Promise<void> {
    await setFolderCover(folderId, itemId);
    await announceUndoableMutation('folders.set_cover');
  },

  async getCoverHashes(folderIds: number[]): Promise<Array<{ folder_id: number; entity_hash: string | null; mime_type: string | null }>> {
    return Promise.all(folderIds.map(async (folderId) => {
      const cover = await getFolderCover(folderId);
      return { folder_id: folderId, entity_hash: cover?.entity_hash ?? null, mime_type: cover?.mime_type ?? null };
    }));
  },

  async setWatchConfig(folderId: number, config: {
    watchPath: string;
    enabled: boolean;
    subfolders: boolean;
    importStatusMode: string;
  }): Promise<void> {
    if (!config.enabled) {
      await clearFolderWatchConfig(folderId);
      await announceUndoableMutation('folders.clear_watch');
      return;
    }
    await setFolderWatchConfig(folderId, config.watchPath, config.subfolders);
    await announceUndoableMutation('folders.set_watch');
  },

  async clearWatchConfig(folderId: number): Promise<void> {
    await clearFolderWatchConfig(folderId);
    await announceUndoableMutation('folders.clear_watch');
  },
};
