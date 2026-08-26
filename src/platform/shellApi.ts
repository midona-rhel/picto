import { invoke } from './ipc';
import type { ResolvedFilePath } from '../shared/types/generated/application/ResolvedFilePath';
import type { ThumbnailQueueResult } from '../shared/types/generated/application/ThumbnailQueueResult';
import type { ItemTarget } from '../shared/types/generated/application/ItemTarget';

export interface AssociatedApplication {
  name: string;
  path: string;
  bundleIdentifier: string;
  iconDataUrl: string | null;
  isDefault: boolean;
}

export interface OpenWithOptions {
  mode: 'submenu' | 'chooser' | 'unsupported';
  applications: AssociatedApplication[];
}

export const REVERSE_IMAGE_SEARCH_ENGINES = [
  { key: 'google', label: 'Google Lens' },
  { key: 'tineye', label: 'TinEye' },
  { key: 'saucenao', label: 'SauceNAO' },
  { key: 'yandex', label: 'Yandex Images' },
  { key: 'sogou', label: 'Sogou' },
  { key: 'bing', label: 'Bing Visual Search' },
] as const;

export type ReverseImageSearchEngine = typeof REVERSE_IMAGE_SEARCH_ENGINES[number]['key'];

export function openExternalUrl(url: string): Promise<void> {
  return invoke<void>('open_external_url', { url });
}

export function openSettingsWindow(): Promise<void> {
  return invoke<void>('open_settings_window');
}

export async function resolveFilePath(fileHash: string): Promise<string> {
  const resolved = await invoke<ResolvedFilePath[]>('media.resolve_paths', {
    file_hashes: [fileHash],
  });
  if (resolved.length !== 1) throw new Error(`Physical file not found: ${fileHash}`);
  return resolved[0].path;
}

export async function reverseImageSearch(
  fileHash: string,
  engine: ReverseImageSearchEngine,
): Promise<void> {
  const filePath = await resolveFilePath(fileHash);
  const search = (window as any).picto?.search?.reverseImage;
  if (typeof search !== 'function') {
    throw new Error('Reverse image search is unavailable.');
  }
  await search(filePath, engine);
}

export type DetailWindowTarget = ({
  hash: string;
  item_id?: never;
} | {
  item_id: number;
  hash?: never;
}) & {
  width?: number | null;
  height?: number | null;
};

export function openDetailWindow(input: DetailWindowTarget): Promise<void> {
  return invoke<void>('open_in_new_window', input as unknown as Record<string, unknown>);
}

export function shellShowInFolder(path: string): void {
  (window as any).picto?.shell?.showInFolder(path);
}

export function shellOpenPath(path: string): void {
  (window as any).picto?.shell?.openPath(path);
}

export function getOpenWithOptions(path: string): Promise<OpenWithOptions> {
  return (window as any).picto?.shell?.getOpenWithOptions(path);
}

export function shellOpenWithApplication(path: string, applicationPath: string): Promise<void> {
  return (window as any).picto?.shell?.openWithApplication(path, applicationPath);
}

export function shellOpenWithChooser(path: string): Promise<void> {
  return (window as any).picto?.shell?.openWithChooser(path);
}

export function clipboardWriteText(text: string): void {
  (window as any).picto?.clipboard?.writeText(text);
}

export function clipboardCopyFile(path: string): Promise<void> {
  return (window as any).picto?.clipboard?.copyFile(path) ?? Promise.resolve();
}

export function clipboardCopyFiles(paths: string[]): Promise<void> {
  return (window as any).picto?.clipboard?.copyFiles(paths) ?? Promise.resolve();
}

export function resolveTargetFilePaths(target: ItemTarget): Promise<ResolvedFilePath[]> {
  return invoke<ResolvedFilePath[]>('media.resolve_target_paths', { target });
}

export function hasClipboardImport(): Promise<boolean> {
  return (window as any).picto?.clipboard?.hasImport() ?? Promise.resolve(false);
}

export function readClipboardImport(): Promise<{ paths: string[]; temporary: boolean }> {
  return (window as any).picto?.clipboard?.readImport()
    ?? Promise.resolve({ paths: [], temporary: false });
}

export function regenerateThumbnailsBatch(fileHashes: string[]): Promise<ThumbnailQueueResult> {
  return invoke<ThumbnailQueueResult>('media.regenerate_thumbnails', {
    file_hashes: fileHashes,
  });
}

export function setThumbnail(fileHash: string, pngBase64: string): Promise<unknown> {
  return invoke('media.set_thumbnail', {
    file_hash: fileHash,
    png_base64: pngBase64,
  });
}
