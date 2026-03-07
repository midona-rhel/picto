import { api } from '#desktop/api';
import { applyMutationEffects } from '../domain/actions/mutationEffects';
import type { SmartFolder } from '../shared/types/api';

/**
 * SmartFolderController — frontend facade for smart folder CRUD.
 */

async function runSmartFolderMutation<T>(promise: Promise<T>): Promise<T> {
  const result = await promise;
  applyMutationEffects({ sidebarTree: true, gridCaches: true });
  return result;
}

export const SmartFolderController = {
  create(folder: SmartFolder | Record<string, unknown>): Promise<SmartFolder> {
    return runSmartFolderMutation(api.smartFolders.create(folder as SmartFolder));
  },

  update(id: number | string, folder: SmartFolder | Record<string, unknown>): Promise<SmartFolder> {
    return runSmartFolderMutation(api.smartFolders.update(String(id), folder as SmartFolder));
  },

  delete(id: number | string): Promise<void> {
    return runSmartFolderMutation(api.smartFolders.delete(String(id)));
  },
};
