import { api } from '#desktop/api';

// Re-export Folder types from central api types for backwards compatibility.
export type { Folder, FolderMembership } from '../types/api';
import type { Folder, FolderMembership, FolderReorderMove } from '../types/api';

/**
 * FolderController — frontend facade for folder CRUD and sidebar reorder commands.
 * Invalidation is handled by mutation receipts via the resource key system.
 */

export const FolderController = {
  createFolder(args: {
    name: string;
    parentId?: number | null;
    icon?: string | null;
    color?: string | null;
  }): Promise<Folder> {
    return (api.folders.create({
      name: args.name,
      parent_id: args.parentId ?? null,
      icon: args.icon ?? undefined,
      color: args.color ?? undefined,
    }));
  },

  updateFolder(args: {
    folderId: number;
    name?: string | null;
    icon?: string | null;
    color?: string | null;
    autoTags?: string[] | null;
  }): Promise<void> {
    return (api.folders.update({
      folder_id: args.folderId,
      name: args.name ?? undefined,
      // Send empty string to explicitly clear icon; undefined means "don't change"
      icon: args.icon === null ? '' : (args.icon === undefined ? undefined : args.icon),
      // Send empty string to explicitly clear color; undefined means "don't change"
      color: args.color === null ? '' : (args.color === undefined ? undefined : args.color),
      auto_tags: args.autoTags ?? undefined,
    }));
  },

  deleteFolder(folderId: number): Promise<void> {
    return (api.folders.delete(folderId));
  },

  // PBI-057: Atomic move_folder — reparent + reorder in one transaction.
  moveFolder(folderId: number, newParentId: number | null, siblingOrder: [number, number][]): Promise<void> {
    return (api.folders.moveFolder(folderId, newParentId, siblingOrder));
  },

  addFileToFolder(folderId: number, hash: string): Promise<void> {
    return (api.folders.addFile(folderId, hash));
  },

  // PBI-054: Batch add files to folder — single IPC call, single event.
  addFilesToFolderBatch(folderId: number, hashes: string[]): Promise<number> {
    return (api.folders.addFilesBatch(folderId, hashes));
  },

  removeFileFromFolder(folderId: number, hash: string): Promise<void> {
    return (api.folders.removeFile(folderId, hash));
  },

  removeFilesFromFolderBatch(folderId: number, hashes: string[]): Promise<number> {
    return (api.folders.removeFilesBatch(folderId, hashes));
  },

  getFolderFiles(folderId: number): Promise<string[]> {
    return api.folders.getFiles(folderId);
  },

  getFolderCoverHash(folderId: number): Promise<string | null> {
    return api.folders.getCoverHash(folderId);
  },

  listFolders(): Promise<Folder[]> {
    return api.folders.list();
  },

  getFileFolders(hash: string): Promise<FolderMembership[]> {
    return api.folders.getFileFolders(hash);
  },

  getEntityFolders(entityId: number): Promise<FolderMembership[]> {
    return api.folders.getEntityFolders(entityId);
  },

  reorderFolders(moves: [number, number][]): Promise<void> {
    return (api.folders.reorder(moves));
  },

  reorderSmartFolders(moves: [number, number][]): Promise<void> {
    return api.smartFolders.reorder(moves);
  },

  reorderFolderItems(
    folderId: number,
    moves: FolderReorderMove[],
  ): Promise<void> {
    return (api.folders.reorderItems(folderId, moves));
  },

  sortFolderItems(
    folderId: number,
    sortBy: string,
    direction: string,
    hashes?: string[],
  ): Promise<void> {
    return (api.folders.sortItems(folderId, sortBy, direction, hashes));
  },

  reverseFolderItems(folderId: number, hashes?: string[]): Promise<void> {
    return (api.folders.reverseItems(folderId, hashes));
  },
};
