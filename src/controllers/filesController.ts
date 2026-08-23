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
import type { BaseScope, EntityTarget } from '../shared/types/canonical';
import type { ExportFormat } from '../shared/types/generated/application/ExportFormat';
import type { ImportEnqueueReport } from '../shared/types/generated/application/ImportEnqueueReport';
import type { Lifecycle } from '../shared/types/generated/application/Lifecycle';

export interface MediaImportParams {
  tags?: string[];
  source_urls?: string[];
  lifecycle: Lifecycle;
  parent_folder_id?: number | null;
  preserve_structure?: boolean;
}

/** Resolve the destination for a manual import from the currently open grid. */
export function manualImportParamsForScope(
  scope: BaseScope,
  params: Omit<MediaImportParams, 'lifecycle'> = {},
): MediaImportParams {
  return {
    ...params,
    lifecycle: scope.kind === 'inbox' ? 'inbox' : 'active',
  };
}

export const filesController = {
  getFolderCoverHash(folderId: number): Promise<string | null> {
    return getFolderCoverHash(folderId);
  },

  addMedia(
    paths: string[],
    params: MediaImportParams,
  ): Promise<ImportEnqueueReport> {
    return addMedia(paths, params);
  },

  exportMedia(
    target: EntityTarget,
    config: {
      output_dir: string;
      format?: ExportFormat | null;
      quality?: number | null;
      width?: number | null;
      height?: number | null;
      keep_aspect?: boolean;
    },
  ): Promise<unknown> {
    return exportMedia(target, config);
  },

  resolveFilePath(hash: string): Promise<string> {
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

  regenerateThumbnailsBatch(hashes: string[]) {
    return regenerateThumbnailsBatch(hashes);
  },
};
