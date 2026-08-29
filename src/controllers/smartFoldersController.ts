/** Smart-folder CRUD through the replacement transaction boundary. */

import { getDefaultStore } from 'jotai';
import {
  createSmartFolder,
  deleteSmartFolder,
  moveSmartFolder,
  reorderSmartFolders,
  updateSmartFolder,
} from '../platform/smartFolderApi';
import type {
  SmartFolderCommandPayload,
} from '../shared/types/canonical';
import { activeNodeIdAtom } from '../state/navigation';
import { pendingSidebarRevealNodeIdAtom } from '../state/sidebar';
import { navigateToNode, removeHistoryEntries } from '../state/navigationHistory';
import { announceUndoableMutation } from '../runtime/historyRuntime';
import { gridController } from './gridController';
import { sidebarController } from './sidebarController';

const store = getDefaultStore();

export function emptySmartFolderPayload(
  overrides: Partial<SmartFolderCommandPayload> = {},
): SmartFolderCommandPayload {
  return {
    name: overrides.name ?? 'New Smart Folder',
    parent_id: overrides.parent_id ?? null,
    icon: overrides.icon ?? null,
    color: overrides.color ?? null,
    notes: overrides.notes ?? null,
    view: overrides.view ?? {
      filter: { kind: 'all', value: [] },
      sort: { field: 'imported_at', direction: 'descending', random_seed: null },
    },
  };
}

export const smartFoldersController = {
  createGroup(name = 'Untitled', parentId: number | null = null): Promise<string> {
    return this.create(emptySmartFolderPayload({ name, parent_id: parentId }));
  },

  async refresh(id: number): Promise<void> {
    await sidebarController.fetchTree();
    if (store.get(activeNodeIdAtom) === `smart:${id}`) {
      await gridController.loadFirstPage({ preserveItems: true });
    }
  },

  async delete(id: string): Promise<void> {
    const smartFolderId = Number(id);
    if (!Number.isSafeInteger(smartFolderId)) throw new Error(`Invalid smart folder ID: ${id}`);
    const result = await deleteSmartFolder(smartFolderId);
    const deleted = new Set(result.deleted_smart_folder_ids.map((value) => `smart:${value}`));
    removeHistoryEntries(deleted);
    if (deleted.has(store.get(activeNodeIdAtom))) {
      const fallback = result.fallback_smart_folder_id == null
        ? 'system:active'
        : `smart:${result.fallback_smart_folder_id}`;
      navigateToNode(fallback);
    }
    await announceUndoableMutation('smart_folders.delete');
  },

  async create(folder: SmartFolderCommandPayload): Promise<string> {
    const result = await createSmartFolder(folder);
    await announceUndoableMutation('smart_folders.create');
    const nodeId = `smart:${result.smart_folder_id}`;
    store.set(pendingSidebarRevealNodeIdAtom, nodeId);
    return nodeId;
  },

  async update(id: number, folder: SmartFolderCommandPayload): Promise<void> {
    await updateSmartFolder(id, folder);
    await announceUndoableMutation('smart_folders.update');
  },

  async preview(id: number, folder: SmartFolderCommandPayload): Promise<void> {
    await updateSmartFolder(id, folder);
    await this.refresh(id);
  },

  async move(smartFolderId: number, parentId: number | null, siblingOrder: [number, number][]) {
    await moveSmartFolder(smartFolderId, parentId);
    if (siblingOrder.length > 0) {
      const orderedIds = [...siblingOrder]
        .sort((left, right) => left[1] - right[1])
        .map(([siblingId]) => siblingId);
      await reorderSmartFolders(parentId, orderedIds);
    }
    await announceUndoableMutation(
      siblingOrder.length > 0 ? 'smart_folders.reorder' : 'smart_folders.move',
    );
  },
};
