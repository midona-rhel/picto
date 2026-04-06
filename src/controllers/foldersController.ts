/**
 * Folder controller — owns folder CRUD actions.
 * Calls API and applies optimistic sidebar updates where the UX benefits.
 */

import { getDefaultStore } from 'jotai';
import {
  clearFolderWatchConfig,
  createFolder,
  deleteFolder,
  getFolderCoverHash,
  importFolder,
  moveFolder,
  renameFolder,
  reorderFolderItems,
  setFolderWatchConfig,
  updateFolder,
} from '../platform/folderApi';
import { removeFolderNodeAtom, patchFolderNodeAtom } from '../state/sidebar';

const store = getDefaultStore();

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
    store.set(removeFolderNodeAtom, folderId);
    await deleteFolder(folderId);
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

  async importFolder(folderPath: string, parentFolderId: number | null) {
    await importFolder(folderPath, {
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
