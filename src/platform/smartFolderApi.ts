import { invoke } from './ipc';
import type { SmartFolderCommandPayload } from '../shared/types/canonical';

export function createSmartFolder(params: {
  folder: SmartFolderCommandPayload;
}): Promise<unknown> {
  return invoke('create_smart_folder', params as unknown as Record<string, unknown>);
}

export function deleteSmartFolder(id: string): Promise<void> {
  return invoke<void>('delete_smart_folder', { id });
}

export function updateSmartFolder(params: {
  id: string;
  folder: SmartFolderCommandPayload;
}): Promise<void> {
  return invoke<void>('update_smart_folder', params);
}

export function moveSmartFolder(
  smartFolderId: number,
  newParentId: number | null,
  siblingOrder: [number, number][],
): Promise<void> {
  return invoke<void>('move_smart_folder', {
    smart_folder_id: smartFolderId,
    new_parent_id: newParentId,
    sibling_order: siblingOrder,
  });
}
