/** Folder CRUD through the replacement transaction and invalidation boundary. */

import { getDefaultStore } from 'jotai';
import {
  addMedia,
  clearFolderWatchConfig,
  createFolder,
  deleteFolder,
  getFolderCoverHash,
  moveFolder,
  renameFolder,
  reorderFolderChildren,
  setFolderMetadata,
  setFolderWatchConfig,
  sortFolderItemsByName,
} from '../platform/folderApi';
import type { FolderMutationReceipt } from '../shared/types/generated/application/FolderMutationReceipt';
import { activeNodeIdAtom } from '../state/navigation';
import { removeHistoryEntries, pushHistory } from '../state/navigationHistory';
import { sidebarNodesAtom } from '../state/sidebar';

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
    store.set(activeNodeIdAtom, fallbackNodeId);
    pushHistory(fallbackNodeId);
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
    return folderNodeId(result.folder_id);
  },

  async rename(folderId: number, newName: string): Promise<void> {
    await renameFolder(folderId, newName);
  },

  async delete(folderId: number): Promise<void> {
    await this.deleteMany([folderId]);
  },

  async deleteMany(folderIds: number[]): Promise<void> {
    const receipts: FolderMutationReceipt[] = [];
    for (const folderId of [...new Set(folderIds)]) {
      receipts.push(await deleteFolder(folderId));
    }
    settleDeletedFolders(receipts);
  },

  async applyColor(folderId: number, color: string | null): Promise<void> {
    await setFolderMetadata({ ...folderMetadata(folderId), color });
  },

  async applyIcon(folderId: number, icon: string | null): Promise<void> {
    await setFolderMetadata({ ...folderMetadata(folderId), icon });
  },

  async applyNotes(folderId: number, notes: string | null): Promise<void> {
    await setFolderMetadata({ ...folderMetadata(folderId), notes });
  },

  async move(folderId: number, parentFolderId: number | null, moves: [number, number][]) {
    await moveFolder(folderId, parentFolderId);
    if (moves.length > 0) {
      const orderedIds = [...moves]
        .sort((left, right) => left[1] - right[1])
        .map(([siblingId]) => siblingId);
      await reorderFolderChildren(parentFolderId, orderedIds);
    }
  },

  async sortByName(folderId: number): Promise<void> {
    await sortFolderItemsByName(folderId);
  },

  async addMedia(folderPath: string, parentFolderId: number | null): Promise<void> {
    await addMedia([folderPath], {
      lifecycle: 'active',
      parent_folder_id: parentFolderId,
      preserve_structure: true,
    });
  },

  getCoverHash(folderId: number): Promise<string | null> {
    return getFolderCoverHash(folderId);
  },

  async setWatchConfig(folderId: number, config: {
    watchPath: string;
    enabled: boolean;
    subfolders: boolean;
    importStatusMode: string;
  }): Promise<void> {
    if (!config.enabled) {
      await clearFolderWatchConfig(folderId);
      return;
    }
    await setFolderWatchConfig(folderId, config.watchPath, config.subfolders);
  },

  async clearWatchConfig(folderId: number): Promise<void> {
    await clearFolderWatchConfig(folderId);
  },
};
