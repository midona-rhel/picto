import { invoke } from './ipc';
import type { CreatedSmartFolder } from '../shared/types/generated/application/CreatedSmartFolder';
import type { SmartFolderMutationReceipt } from '../shared/types/generated/application/SmartFolderMutationReceipt';
import type { SmartFolderCommandPayload } from '../shared/types/canonical';

export function createSmartFolder(input: SmartFolderCommandPayload): Promise<CreatedSmartFolder> {
  return invoke<CreatedSmartFolder>('smart_folders.create', { ...input });
}

export function deleteSmartFolder(smartFolderId: number): Promise<SmartFolderMutationReceipt> {
  return invoke<SmartFolderMutationReceipt>('smart_folders.delete', {
    smart_folder_id: smartFolderId,
  });
}

export function updateSmartFolder(
  smartFolderId: number,
  value: SmartFolderCommandPayload,
): Promise<SmartFolderMutationReceipt> {
  return invoke<SmartFolderMutationReceipt>('smart_folders.update', {
    smart_folder_id: smartFolderId,
    value,
  });
}

export function moveSmartFolder(
  smartFolderId: number,
  parentId: number | null,
): Promise<SmartFolderMutationReceipt> {
  return invoke<SmartFolderMutationReceipt>('smart_folders.move', {
    smart_folder_id: smartFolderId,
    parent_id: parentId,
  });
}

export function reorderSmartFolders(
  parentId: number | null,
  smartFolderIds: number[],
): Promise<SmartFolderMutationReceipt> {
  return invoke<SmartFolderMutationReceipt>('smart_folders.reorder', {
    parent_id: parentId,
    smart_folder_ids: smartFolderIds,
  });
}
