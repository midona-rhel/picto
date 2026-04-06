import { invoke } from './ipc';

export interface EntityAssetResult {
  role: string;
  available: boolean;
  url?: string | null;
  mime_type?: string | null;
  path?: string | null;
  source_entity_hash?: string | null;
}

export function openExternalUrl(url: string): Promise<void> {
  return invoke<void>('open_external_url', { url });
}

export function openSettingsWindow(): Promise<void> {
  return invoke<void>('open_settings_window');
}

export function resolveEntityAsset(hash: string, role: string): Promise<EntityAssetResult> {
  return invoke<EntityAssetResult>('resolve_entity_asset', { entity_hash: hash, role });
}

export function resolveFilePath(hash: string): Promise<string | null> {
  return invoke<string | null>('resolve_file_path', { hash });
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

export function clipboardWriteText(text: string): void {
  (window as any).picto?.clipboard?.writeText(text);
}

export function clipboardCopyFile(path: string): void {
  (window as any).picto?.clipboard?.copyFile(path);
}

export function regenerateThumbnailsBatch(hashes: string[]): Promise<{ total: number; regenerated: number; errors: number }> {
  return invoke('regenerate_thumbnails_batch', { hashes } as unknown as Record<string, unknown>);
}
