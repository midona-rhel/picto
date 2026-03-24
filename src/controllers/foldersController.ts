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
  async create(name: string, parentId?: number | null) {
    const result = await api.createFolder({ name, parent_id: parentId ?? null });
    // Refresh tree to get the new node from the backend
    await sidebarController.fetchTree();
    return result;
  },

  async rename(folderId: number, newName: string) {
    await api.renameFolder(folderId, newName);
    store.set(patchFolderNodeAtom, { folderId, patch: { name: newName } });
  },

  async delete(folderId: number) {
    await api.deleteFolder(folderId);
    store.set(removeFolderNodeAtom, folderId);
  },
};
