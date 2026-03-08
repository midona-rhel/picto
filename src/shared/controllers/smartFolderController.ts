import { api } from '#desktop/api';
import type { SmartFolder, SmartFolderIpcInput } from '../types/api';

/**
 * SmartFolderController — frontend facade for smart folder CRUD.
 * Invalidation is handled by mutation receipts via the resource key system.
 */

export const SmartFolderController = {
  create(folder: SmartFolderIpcInput): Promise<SmartFolder> {
    return api.smartFolders.create(folder);
  },

  update(id: number | string, folder: SmartFolderIpcInput): Promise<SmartFolder> {
    return api.smartFolders.update(String(id), folder);
  },

  delete(id: number | string): Promise<void> {
    return api.smartFolders.delete(String(id));
  },
};
