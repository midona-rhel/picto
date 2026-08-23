import { invoke } from './ipc';
import type { EntityTarget } from '../shared/types/canonical';

export function createFolder(params: {
  name: string;
  parent_id?: number | null;
  icon?: string;
  color?: string;
}): Promise<unknown> {
  return invoke('create_folder', params);
}

export function deleteFolder(folderId: number): Promise<void> {
  return invoke<void>('delete_folder', { folder_id: folderId });
}

export function removeEntitiesFromFolder(folderId: number, target: EntityTarget): Promise<void> {
  return invoke<void>('remove_entities_from_folder', { folder_id: folderId, target });
}

export function renameFolder(folderId: number, name: string): Promise<void> {
  return invoke<void>('update_folder', { folder_id: folderId, name });
}

export function updateFolder(folderId: number, patch: {
  name?: string;
  icon?: string | null;
  color?: string | null;
  notes?: string | null;
}): Promise<void> {
  return invoke<void>('update_folder', { folder_id: folderId, ...patch });
}

export function moveFolder(
  folderId: number,
  newParentId: number | null,
  siblingOrder: [number, number][],
): Promise<void> {
  return invoke<void>('move_folder', {
    folder_id: folderId,
    new_parent_id: newParentId,
    sibling_order: siblingOrder,
  });
}

export function updateFolderMembership(
  target: EntityTarget,
  folderId: number,
  operation: 'add' | 'remove',
): Promise<unknown> {
  return invoke('update_folder_membership', {
    target,
    folder_id: folderId,
    operation,
  } as unknown as Record<string, unknown>);
}

export function setFolderWatchConfig(folderId: number, config: {
  watch_path: string;
  watch_enabled: boolean;
  watch_subfolders: boolean;
  watch_import_status_mode: string;
}): Promise<void> {
  return invoke<void>('set_folder_watch_config', { folder_id: folderId, ...config });
}

export function clearFolderWatchConfig(folderId: number): Promise<void> {
  return invoke<void>('clear_folder_watch_config', { folder_id: folderId });
}

export interface FolderCoverHashDto {
  folder_id: number;
  entity_hash: string | null;
}

export function getFolderCoverHashes(folderIds: number[]): Promise<FolderCoverHashDto[]> {
  return invoke<FolderCoverHashDto[]>('get_folder_cover_hashes', { folder_ids: folderIds });
}

export function addMedia(paths: string[], params?: {
  tag_strings?: string[];
  source_urls?: string[];
  initial_status?: number;
  parent_folder_id?: number | null;
  preserve_structure?: boolean;
}): Promise<void> {
  return invoke<void>('add_media', {
    paths,
    ...params,
    preserve_structure: params?.preserve_structure ?? false,
    parent_folder_id: params?.parent_folder_id ?? null,
  } as unknown as Record<string, unknown>);
}

export function exportMedia(target: EntityTarget, config: {
  output_dir: string;
  format?: string | null;
  quality?: number | null;
  width?: number | null;
  height?: number | null;
  keep_aspect?: boolean;
}): Promise<unknown> {
  return invoke('export_media', { target, ...config } as unknown as Record<string, unknown>);
}

export function reorderFolderItems(folderId: number, params: {
  sort_by?: string;
  direction?: string;
  moves?: FolderReorderMove[];
  hashes?: string[];
}): Promise<void> {
  return invoke<void>('reorder_folder_items', { folder_id: folderId, ...params } as unknown as Record<string, unknown>);
}

export interface FolderReorderMove {
  hash: string;
  before_hash?: string | null;
  after_hash?: string | null;
}
