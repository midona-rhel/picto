/**
 * Folder controller — owns folder CRUD actions.
 * Calls API, then eagerly updates sidebar state atoms.
 */

import { getDefaultStore } from 'jotai';
import * as api from '../platform/api';
import { removeFolderNodeAtom, patchFolderNodeAtom } from '../state/sidebar';
import { sidebarController } from './sidebarController';

const store = getDefaultStore();

export const foldersController = {
  async create(name: string, parentId?: number | null): Promise<string | null> {
    const result = await api.createFolder({ name, parent_id: parentId ?? null });
    await sidebarController.fetchTree();
    // Return the node ID if the backend returned a folder_id
    const folderId = result && typeof result === 'object' && 'folder_id' in result
      ? (result as { folder_id: number }).folder_id
      : null;
    return folderId != null ? `folder:${folderId}` : null;
  },

  async rename(folderId: number, newName: string) {
    store.set(patchFolderNodeAtom, { folderId, patch: { name: newName } });
    await api.renameFolder(folderId, newName);
  },

  async delete(folderId: number) {
    store.set(removeFolderNodeAtom, folderId);
    await api.deleteFolder(folderId);
  },

  async applyColor(folderId: number, color: string | null) {
    store.set(patchFolderNodeAtom, { folderId, patch: { color } });
    await api.updateFolder(folderId, { color });
  },

  async applyIcon(folderId: number, icon: string | null) {
    store.set(patchFolderNodeAtom, { folderId, patch: { icon } });
    await api.updateFolder(folderId, { icon });
  },
};
