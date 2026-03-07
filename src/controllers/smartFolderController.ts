import { api } from '#desktop/api';
import { applyMutationEffects } from '../domain/actions/mutationEffects';
import type { SmartFolder, SmartFolderIpcInput } from '../shared/types/api';

/**
 * SmartFolderController — frontend facade for smart folder CRUD.
 */

async function runSmartFolderMutation<T>(promise: Promise<T>): Promise<T> {
  const result = await promise;
  applyMutationEffects({ sidebarTree: true, gridCaches: true });
  return result;
}

export const SmartFolderController = {
  create(folder: SmartFolderIpcInput): Promise<SmartFolder> {
    return runSmartFolderMutation(api.smartFolders.create(folder));
  },

  update(id: number | string, folder: SmartFolderIpcInput): Promise<SmartFolder> {
    return runSmartFolderMutation(api.smartFolders.update(String(id), folder));
  },

  delete(id: number | string): Promise<void> {
    return runSmartFolderMutation(api.smartFolders.delete(String(id)));
  },
};
