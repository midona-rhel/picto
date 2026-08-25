import { invoke } from './ipc';
import type { ResolvedFilePath } from '../shared/types/generated/application/ResolvedFilePath';
import type { ThumbnailQueueResult } from '../shared/types/generated/application/ThumbnailQueueResult';

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

export function openDetailWindow(input: {
  hash: string;
  width?: number | null;
  height?: number | null;
}): Promise<void> {
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

export function clipboardCopyFile(path: string): void {
  (window as any).picto?.clipboard?.copyFile(path);
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
