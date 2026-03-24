import { api } from '#desktop/api';
import { canStartTaskFamily, useTaskStore, type TaskProgress } from '../state-legacy/taskStore';

export const importController = {
  canStart(): { allowed: boolean; reason?: string } {
    return canStartTaskFamily('import');
  },

  /** Called by event listeners when backend sends import progress. */
  updateProgress(progress: TaskProgress) {
    const store = useTaskStore.getState();
    if (!store.familyProgress.import.running) {
      store.startFamily('import');
    }
    store.updateFamilyProgress('import', progress);
  },

  /** Called when import finishes (success or fail). */
  finish() {
    useTaskStore.getState().finishFamily('import');
  },

  importFiles(paths: string[], tagStrings?: string[], sourceUrls?: string[], initialStatus?: number) {
    const check = canStartTaskFamily('import');
    if (!check.allowed) {
      return Promise.reject(new Error(check.reason ?? 'Import blocked'));
    }
    useTaskStore.getState().startFamily('import');
    return api.import.files(paths, tagStrings, sourceUrls, initialStatus);
  },

  importFolder(path: string, preserveStructure: boolean, parentFolderId?: number | null, initialStatus?: number) {
    const check = canStartTaskFamily('import');
    if (!check.allowed) {
      return Promise.reject(new Error(check.reason ?? 'Import blocked'));
    }
    useTaskStore.getState().startFamily('import');
    return api.import.folder(path, preserveStructure, parentFolderId, initialStatus);
  },
};
