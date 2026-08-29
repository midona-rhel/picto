import { queryItems } from './entityApi';
import { invoke } from './ipc';
import type { CreatedFolder } from '../shared/types/generated/application/CreatedFolder';
import type { ExportFormat } from '../shared/types/generated/application/ExportFormat';
import type { ExportResult } from '../shared/types/generated/application/ExportResult';
import type { FolderMutationReceipt } from '../shared/types/generated/application/FolderMutationReceipt';
import type { ImportEnqueueReport } from '../shared/types/generated/application/ImportEnqueueReport';
import type { EntityTarget, Lifecycle } from '../shared/types/canonical';
import { compileGridQuery, createEmptyItemFilters } from '../shared/lib/itemFilters';

export function createFolder(params: {
  name: string;
  parent_id?: number | null;
}): Promise<CreatedFolder> {
  return invoke<CreatedFolder>('folders.create', {
    name: params.name,
    parent_id: params.parent_id ?? null,
    folder_key: null,
  });
}

export function duplicateFolder(folderId: number): Promise<CreatedFolder> {
  return invoke<CreatedFolder>('folders.duplicate', { folder_id: folderId });
}

export function deleteFolders(folderIds: number[]): Promise<FolderMutationReceipt> {
  return invoke<FolderMutationReceipt>('folders.delete', { folder_ids: folderIds });
}

export function removeEntitiesFromFolder(folderId: number, target: EntityTarget): Promise<unknown> {
  return invoke('items.set_folder', { folder_id: folderId, target, present: false });
}

export function renameFolder(folderId: number, name: string): Promise<FolderMutationReceipt> {
  return invoke<FolderMutationReceipt>('folders.rename', { folder_id: folderId, name });
}

export function setFolderMetadata(input: {
  folder_id: number;
  icon: string | null;
  color: string | null;
  notes: string | null;
}): Promise<FolderMutationReceipt> {
  return invoke<FolderMutationReceipt>('folders.metadata.set', input);
}

export function getFolderAutoTags(folderId: number): Promise<string[]> {
  return invoke<string[]>('folders.auto_tags.get', { folder_id: folderId });
}

export function setFolderAutoTags(folderId: number, tags: string[]): Promise<FolderMutationReceipt> {
  return invoke<FolderMutationReceipt>('folders.auto_tags.set', { folder_id: folderId, tags });
}

export function moveFolder(folderId: number, parentId: number | null): Promise<FolderMutationReceipt> {
  return invoke<FolderMutationReceipt>('folders.move', {
    folder_id: folderId,
    parent_id: parentId,
  });
}

export function reorderFolderChildren(
  parentId: number | null,
  folderIds: number[],
): Promise<FolderMutationReceipt> {
  return invoke<FolderMutationReceipt>('folders.reorder', {
    parent_id: parentId,
    folder_ids: folderIds,
  });
}

export function sortFolderTree(
  folderId: number,
  descending: boolean,
  recursive: boolean,
): Promise<FolderMutationReceipt> {
  return invoke<FolderMutationReceipt>('folders.sort_tree', {
    folder_id: folderId,
    descending,
    recursive,
  });
}

export function updateFolderMembership(
  target: EntityTarget,
  folderId: number,
  operation: 'add' | 'remove',
): Promise<unknown> {
  return invoke('items.set_folder', {
    target,
    folder_id: folderId,
    present: operation === 'add',
  });
}

export function setFolderWatchConfig(
  folderId: number,
  path: string,
  includeSubfolders: boolean,
): Promise<FolderMutationReceipt> {
  return invoke<FolderMutationReceipt>('folders.watch.set', {
    folder_id: folderId,
    path,
    include_subfolders: includeSubfolders,
  });
}

export function clearFolderWatchConfig(folderId: number): Promise<FolderMutationReceipt> {
  return invoke<FolderMutationReceipt>('folders.watch.clear', { folder_id: folderId });
}

export async function getFolderCover(folderId: number): Promise<{ entity_hash: string; mime_type: string } | null> {
  const explicit = await invoke<{ entity_hash: string; mime_type: string } | null>(
    'folders.cover.get',
    { folder_id: folderId },
  );
  if (explicit) return explicit;
  const page = await queryItems(compileGridQuery(
    { kind: 'folder', folder_id: folderId },
    createEmptyItemFilters(),
    { field: 'folder_order', direction: 'ascending', random_seed: null },
  ), { cursor: null, limit: 1 });
  const item = page.items[0];
  return item ? { entity_hash: item.content_hash, mime_type: item.mime } : null;
}

export function setFolderCover(folderId: number, itemId: number): Promise<FolderMutationReceipt> {
  return invoke<FolderMutationReceipt>('folders.cover.set', {
    folder_id: folderId,
    item_id: itemId,
  });
}

export function addMedia(paths: string[], params: {
  tags?: string[];
  source_urls?: string[];
  lifecycle: Lifecycle;
  parent_folder_id?: number | null;
  preserve_structure?: boolean;
  include_subfolders?: boolean;
  expand_archives?: boolean;
  include_folders_without_media?: boolean;
  delete_after_ingest?: boolean;
  group_files?: boolean;
}): Promise<ImportEnqueueReport> {
  return invoke<ImportEnqueueReport>('imports.enqueue', {
    paths,
    tags: params.tags ?? [],
    source_urls: params.source_urls ?? [],
    lifecycle: params.lifecycle,
    parent_folder_id: params.parent_folder_id ?? null,
    preserve_structure: params.preserve_structure ?? false,
    include_subfolders: params.include_subfolders ?? true,
    expand_archives: params.expand_archives ?? true,
    include_folders_without_media: params.include_folders_without_media ?? false,
    delete_after_ingest: params.delete_after_ingest ?? false,
    group_files: params.group_files ?? false,
  });
}

export function exportMedia(target: EntityTarget, config: {
  output_dir: string;
  format?: ExportFormat | null;
  quality?: number | null;
  width?: number | null;
  height?: number | null;
  keep_aspect?: boolean;
}): Promise<ExportResult> {
  return invoke<ExportResult>('media.export', {
    target,
    output_dir: config.output_dir,
    format: config.format ?? 'original',
    quality: config.quality ?? 90,
    width: config.width ?? null,
    height: config.height ?? null,
    keep_aspect: config.keep_aspect ?? true,
  });
}

export function reorderFolderMembers(
  folderId: number,
  itemIds: number[],
): Promise<FolderMutationReceipt> {
  return invoke<FolderMutationReceipt>('folders.items.reorder', {
    folder_id: folderId,
    item_ids: itemIds,
  });
}

export type ContentSortField = 'name' | 'imported_at' | 'created_at' | 'modified_at' | 'size' | 'notes';

export function sortFolderItems(folderId: number, field: ContentSortField): Promise<FolderMutationReceipt> {
  return invoke<FolderMutationReceipt>('folders.items.sort', { folder_id: folderId, field });
}
