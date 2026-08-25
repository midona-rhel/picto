import {
  exportMedia,
  addMedia,
} from '../platform/folderApi';
import {
  clipboardCopyFile,
  clipboardWriteText,
  hasClipboardImport,
  readClipboardImport,
  regenerateThumbnailsBatch,
  setThumbnail,
  resolveFilePath,
  getOpenWithOptions,
  shellOpenPath,
  shellOpenWithApplication,
  shellOpenWithChooser,
  shellShowInFolder,
} from '../platform/shellApi';
import type { BaseScope, EntityTarget } from '../shared/types/canonical';
import type { ExportFormat } from '../shared/types/generated/application/ExportFormat';
import type { ImportEnqueueReport } from '../shared/types/generated/application/ImportEnqueueReport';
import type { Lifecycle } from '../shared/types/generated/application/Lifecycle';
import { getDefaultStore } from 'jotai';
import { multiFileImportModalAtom } from '../state/modals';
import { showErrorNotification } from '../shared/lib/notifications';

export interface MediaImportParams {
  tags?: string[];
  source_urls?: string[];
  lifecycle: Lifecycle;
  parent_folder_id?: number | null;
  preserve_structure?: boolean;
  delete_after_ingest?: boolean;
  group_files?: boolean;
}

const store = getDefaultStore();

function enqueueMediaImport(paths: string[], params: MediaImportParams): void {
  void addMedia(paths, params).catch((reason) => {
    showErrorNotification({
      title: 'Could not import media',
      message: reason instanceof Error ? reason.message : String(reason),
    });
  });
}

/** Import one file directly or ask how a multi-file selection should be represented. */
export function requestMediaImport(paths: string[], params: MediaImportParams): void {
  if (paths.length <= 1) {
    enqueueMediaImport(paths, params);
    return;
  }
  store.set(multiFileImportModalAtom, {
    open: true,
    paths,
    lifecycle: params.lifecycle,
    parentFolderId: params.parent_folder_id ?? null,
    tags: params.tags ?? [],
    sourceUrls: params.source_urls ?? [],
    preserveStructure: params.preserve_structure ?? false,
    deleteAfterIngest: params.delete_after_ingest ?? false,
  });
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

const MEDIA_IMPORT_FILTERS = [{
  name: 'Media',
  extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'mp4', 'webm', 'mkv', 'mov', 'avi'],
}];

/** Use the canonical desktop picker and import destination for a grid scope. */
export async function chooseAndImportFiles(scope: BaseScope): Promise<void> {
  const result = await (window as any).picto.dialog.open({
    properties: ['openFile'],
    multiple: true,
    title: 'Import files',
    filters: MEDIA_IMPORT_FILTERS,
  });
  if (!result) return;
  const paths = Array.isArray(result) ? result : [result];
  requestMediaImport(paths, manualImportParamsForScope(
    scope,
    scope.kind === 'folder' ? { parent_folder_id: scope.folder_id } : {},
  ));
}

/** Use the canonical desktop picker and structure-preserving folder import. */
export async function chooseAndImportFolder(scope: BaseScope): Promise<void> {
  const result = await (window as any).picto.dialog.open({
    properties: ['openDirectory'],
    multiple: false,
    title: 'Import folder',
  });
  if (!result) return;
  const folderPath = typeof result === 'string' ? result : result[0];
  if (!folderPath) return;
  await addMedia([folderPath], manualImportParamsForScope(scope, {
    preserve_structure: true,
    parent_folder_id: scope.kind === 'folder' ? scope.folder_id : null,
  }));
}

/** Import copied files or a copied bitmap through the durable ingest queue. */
export async function pasteImport(scope: BaseScope): Promise<void> {
  const payload = await readClipboardImport();
  if (payload.paths.length === 0) {
    throw new Error('The clipboard does not contain importable files or an image.');
  }
  const params = manualImportParamsForScope(scope, {
    parent_folder_id: scope.kind === 'folder' ? scope.folder_id : null,
    delete_after_ingest: payload.temporary,
  });
  if (payload.paths.length > 1) {
    requestMediaImport(payload.paths, params);
    return;
  }
  await addMedia(payload.paths, params);
}

/** Pick one destination and export the target's original files unchanged. */
export async function chooseAndExportOriginals(target: EntityTarget): Promise<void> {
  const result = await (window as any).picto.dialog.open({
    properties: ['openDirectory'],
    multiple: false,
    title: 'Export originals',
  });
  const outputDir = typeof result === 'string' ? result : result?.[0];
  if (!outputDir) return;
  await exportMedia(target, { output_dir: outputDir, format: 'original' });
}

export { hasClipboardImport };

export const filesController = {
  chooseAndExportOriginals,
  addMedia(
    paths: string[],
    params: MediaImportParams,
  ): Promise<ImportEnqueueReport> {
    return addMedia(paths, params);
  },

  requestMediaImport,

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

  async getOpenWithOptionsForHash(hash: string) {
    const path = await resolveFilePath(hash);
    return getOpenWithOptions(path);
  },

  async openWithApplicationForHash(hash: string, applicationPath: string): Promise<void> {
    const path = await resolveFilePath(hash);
    await shellOpenWithApplication(path, applicationPath);
  },

  async openWithChooserForHash(hash: string): Promise<void> {
    const path = await resolveFilePath(hash);
    await shellOpenWithChooser(path);
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

  setThumbnail(hash: string, pngDataUrl: string) {
    const comma = pngDataUrl.indexOf(',');
    if (!pngDataUrl.startsWith('data:image/png;base64,') || comma < 0) {
      return Promise.reject(new Error('Thumbnail capture did not produce a PNG.'));
    }
    return setThumbnail(hash, pngDataUrl.slice(comma + 1));
  },
};
