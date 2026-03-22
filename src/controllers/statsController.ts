import { api } from '#desktop/api';
import type { FileStats, PerfSloResult } from '../shared/types/api';

export const statsController = {
  getImageStorageStats(): Promise<FileStats> {
    return api.stats.getImageStorageStats();
  },

  checkPerfSlo(): Promise<PerfSloResult> {
    return api.stats.checkPerfSlo();
  },
};
