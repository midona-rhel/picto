import {
  exportMedia,
  getFolderCoverHash,
  addMedia,
} from '../platform/folderApi';
import {
  clipboardCopyFile,
  clipboardWriteText,
  regenerateThumbnailsBatch,
  resolveFilePath,
  shellOpenPath,
  shellShowInFolder,
} from '../platform/shellApi';
import type { EntityTarget } from '../shared/types/canonical';

export const filesController = {
  getFolderCoverHash(folderId: number): Promise<string | null> {
    return getFolderCoverHash(folderId);
  },

  addMedia(
    paths: string[],
    params?: {
      tag_strings?: string[];
      source_urls?: string[];
      initial_status?: number;
      parent_folder_id?: number | null;
      preserve_structure?: boolean;
      collection_name?: string | null;
    },
  ): Promise<void> {
    return addMedia(paths, params);
  },

  exportMedia(
    target: EntityTarget,
    config: {
      output_dir: string;
      format?: string | null;
      quality?: number | null;
      width?: number | null;
      height?: number | null;
      keep_aspect?: boolean;
    },
  ): Promise<unknown> {
    return exportMedia(target, config);
  },

  resolveFilePath(hash: string): Promise<string | null> {
    return resolveFilePath(hash);
  },

  openPath(path: string): void {
    shellOpenPath(path);
  },

  showInFolder(path: string): void {
    shellShowInFolder(path);
  },

  copyText(text: string): void {
    clipboardWriteText(text);
  },

  copyFile(path: string): void {
    clipboardCopyFile(path);
  },

  async openDefaultAppForHash(hash: string): Promise<void> {
    const path = await resolveFilePath(hash);
    if (path) shellOpenPath(path);
  },

  async revealHashInFolder(hash: string): Promise<void> {
    const path = await resolveFilePath(hash);
    if (path) shellShowInFolder(path);
  },

  async copyFilePath(hash: string): Promise<void> {
    const path = await resolveFilePath(hash);
    if (path) clipboardWriteText(path);
  },

  async copyFileForHash(hash: string): Promise<void> {
    const path = await resolveFilePath(hash);
    if (path) clipboardCopyFile(path);
  },

  regenerateThumbnailsBatch(hashes: string[]): Promise<{ total: number; regenerated: number; errors: number }> {
    return regenerateThumbnailsBatch(hashes);
  },
};
