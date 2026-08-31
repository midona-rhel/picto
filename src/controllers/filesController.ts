import {
  exportMedia,
  addMedia as enqueueImport,
} from '../platform/folderApi';
import {
  clipboardCopyFile,
  clipboardCopyFiles,
  clipboardWriteText,
  hasClipboardImport,
  readClipboardImport,
  regenerateThumbnailsBatch,
  setThumbnail,
  resolveFilePath,
  resolveTargetFilePaths,
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
import { folderImportModalAtom, multiFileImportModalAtom, pictoPackModalAtom } from '../state/modals';
import { showErrorNotification } from '../shared/lib/notifications';
import { getSettings, type AppSettings } from '../platform/settingsApi';
import { t } from '../i18n';
import { inspectPictoPack, type PictoPackSource } from '../platform/pictoPackApi';
import { recordRecentFolderUse } from '../shared/hooks/useRecentFolders';

export interface MediaImportParams {
  tags?: string[];
  source_urls?: string[];
  lifecycle: Lifecycle;
  parent_folder_id?: number | null;
  preserve_structure?: boolean;
  include_subfolders?: boolean;
  include_folders_without_media?: boolean;
  watch_source_folder?: boolean;
  delete_after_ingest?: boolean;
  group_files?: boolean;
}

const store = getDefaultStore();

async function addMedia(paths: string[], params: MediaImportParams): Promise<ImportEnqueueReport> {
  const report = await enqueueImport(paths, params);
  if (params.parent_folder_id != null) recordRecentFolderUse([params.parent_folder_id]);
  return report;
}

function enqueueMediaImport(paths: string[], params: MediaImportParams): void {
  void addMedia(paths, params).catch((reason) => {
    showErrorNotification({
      title: t("Could not import media"),
      message: reason instanceof Error ? reason.message : String(reason),
    });
  });
}

/** Resolve one multi-file batch through the user's single persisted import policy. */
export async function requestMediaImport(paths: string[], params: MediaImportParams): Promise<void> {
  if (paths.length <= 1 || params.group_files != null) {
    enqueueMediaImport(paths, params);
    return;
  }

  const behavior: AppSettings['multiFileImportBehavior'] = await getSettings()
    .then((settings) => settings.multiFileImportBehavior)
    .catch(() => 'ask');
  if (behavior !== 'ask') {
    enqueueMediaImport(paths, { ...params, group_files: behavior === 'group' });
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
    title: t("Import files"),
    filters: MEDIA_IMPORT_FILTERS,
  });
  if (!result) return;
  const paths = Array.isArray(result) ? result : [result];
  await requestMediaImport(paths, manualImportParamsForScope(
    scope,
    scope.kind === 'folder' ? { parent_folder_id: scope.folder_id } : {},
  ));
}

/** Use the canonical desktop picker and structure-preserving folder import. */
export async function chooseAndImportFolder(scope: BaseScope): Promise<void> {
  const result = await (window as any).picto.dialog.open({
    properties: ['openDirectory'],
    multiple: false,
    title: t("Import folder"),
  });
  if (!result) return;
  const folderPath = typeof result === 'string' ? result : result[0];
  if (!folderPath) return;
  store.set(folderImportModalAtom, {
    open: true,
    path: folderPath,
    targetFolderId: scope.kind === 'folder' ? scope.folder_id : null,
    lifecycle: manualImportParamsForScope(scope).lifecycle,
  });
}

export function requestPictoPackExport(
  source: PictoPackSource,
  itemCount: number,
  suggestedName = 'Picto Pack',
): void {
  store.set(pictoPackModalAtom, { open: true, mode: 'export', source, itemCount, suggestedName });
}

export function pictoPackPathFromDrop(paths: string[]): string | null {
  const packPaths = paths.filter((path) => /\.picto-pack$/i.test(path));
  if (packPaths.length === 0) return null;
  if (paths.length !== 1) {
    throw new Error(t("Drop one Picto Pack at a time without other files."));
  }
  return packPaths[0];
}

export async function openPictoPackImport(path: string): Promise<void> {
  const summary = await inspectPictoPack(path);
  store.set(pictoPackModalAtom, { open: true, mode: 'import', path, summary });
}

export async function chooseAndImportPictoPack(): Promise<void> {
  const result = await (window as any).picto.dialog.open({
    properties: ['openFile'],
    multiple: false,
    title: t("Import Picto Pack"),
    filters: [{ name: t("Picto Pack"), extensions: ['picto-pack'] }],
  });
  const path = typeof result === 'string' ? result : result?.[0];
  if (!path) return;
  await openPictoPackImport(path);
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
    await requestMediaImport(payload.paths, params);
    return;
  }
  await addMedia(payload.paths, params);
}

/** Pick one destination and export the target's original files unchanged. */
export async function chooseAndExportOriginals(target: EntityTarget): Promise<void> {
  const result = await (window as any).picto.dialog.open({
    properties: ['openDirectory'],
    multiple: false,
    title: t("Export originals"),
  });
  const outputDir = typeof result === 'string' ? result : result?.[0];
  if (!outputDir) return;
  await exportMedia(target, { output_dir: outputDir, format: 'original' });
}

export { hasClipboardImport };

export const filesController = {
  chooseAndExportOriginals,
  chooseAndImportPictoPack,
  openPictoPackImport,
  pictoPackPathFromDrop,
  requestPictoPackExport,
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

  copyFile(path: string): Promise<void> {
    return clipboardCopyFile(path);
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
    if (path) await clipboardCopyFile(path);
  },

  async copyHashes(hashes: string[]): Promise<void> {
    const paths = await Promise.all(hashes.map(resolveFilePath));
    if (paths.length === 0) throw new Error('The selection has no physical files to copy.');
    await clipboardCopyFiles(paths);
  },

  async copyHashPaths(hashes: string[]): Promise<void> {
    const paths = await Promise.all(hashes.map(resolveFilePath));
    if (paths.length === 0) throw new Error('The selection has no physical file paths to copy.');
    clipboardWriteText(paths.join('\n'));
  },

  async copyHashLinks(hashes: string[]): Promise<void> {
    const paths = await Promise.all(hashes.map(resolveFilePath));
    clipboardWriteText(paths.map((path, index) => {
      const extension = path.split(/[\\/]/).pop()?.split('.').pop() ?? 'bin';
      return `media://localhost/file/${hashes[index]}.${extension}`;
    }).join('\n'));
  },

  async copyTarget(target: EntityTarget): Promise<void> {
    const resolved = await resolveTargetFilePaths(target);
    if (resolved.length === 0) throw new Error('The selection has no physical files to copy.');
    await clipboardCopyFiles(resolved.map((entry) => entry.path));
  },

  async copyTargetPaths(target: EntityTarget): Promise<void> {
    const resolved = await resolveTargetFilePaths(target);
    if (resolved.length === 0) throw new Error('The selection has no physical file paths to copy.');
    clipboardWriteText(resolved.map((entry) => entry.path).join('\n'));
  },

  async copyTargetLinks(target: EntityTarget): Promise<void> {
    const resolved = await resolveTargetFilePaths(target);
    if (resolved.length === 0) throw new Error('The selection has no physical files to link.');
    clipboardWriteText(resolved.map((entry) => {
      const pathParts = entry.path.split(/[\\/]/);
      const filename = pathParts[pathParts.length - 1] ?? '';
      const nameParts = filename.split('.');
      const extension = nameParts.length > 1 ? nameParts[nameParts.length - 1] : 'bin';
      return `media://localhost/file/${entry.file_hash}.${extension}`;
    }).join('\n'));
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
