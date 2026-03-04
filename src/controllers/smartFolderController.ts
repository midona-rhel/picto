import { api } from '#desktop/api';
import type { SmartFolder } from '../types/api';

/**
 * SmartFolderController — frontend facade for smart folder CRUD.
 */
export const SmartFolderController = {
  create(folder: SmartFolder | Record<string, unknown>): Promise<SmartFolder> {
    return api.smartFolders.create(folder as SmartFolder);
  },

  update(id: number | string, folder: SmartFolder | Record<string, unknown>): Promise<SmartFolder> {
    return api.smartFolders.update(String(id), folder as SmartFolder);
  },

  delete(id: number | string): Promise<void> {
    return api.smartFolders.delete(String(id));
  },
};
