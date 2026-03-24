import { api } from '#desktop/api';
import type { ExportMediaInput, ExportMediaResult } from '../shared/types/generated/commands';
import { canStartTaskFamily, useTaskStore, type TaskProgress } from '../state-legacy/taskStore';

export const exportController = {
  canStart(): { allowed: boolean; reason?: string } {
    return canStartTaskFamily('export');
  },

  /** Called by event listeners when backend sends export progress. */
  updateProgress(progress: TaskProgress) {
    const store = useTaskStore.getState();
    if (!store.familyProgress.export.running) {
      store.startFamily('export');
    }
    store.updateFamilyProgress('export', progress);
  },

  finish() {
    useTaskStore.getState().finishFamily('export');
    useTaskStore.getState().setExportRunning(false);
  },

  async run(input: ExportMediaInput): Promise<ExportMediaResult> {
    const check = canStartTaskFamily('export');
    if (!check.allowed) {
      throw new Error(check.reason ?? 'Export blocked');
    }
    useTaskStore.getState().startFamily('export');
    useTaskStore.getState().setExportRunning(true);
    try {
      const result = await api.export.run(input);
      return result;
    } catch (err) {
      useTaskStore.getState().failFamily('export');
      useTaskStore.getState().setExportRunning(false);
      throw err;
    }
  },
};
