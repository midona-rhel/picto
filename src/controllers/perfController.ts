import { api } from '#desktop/api';
import type { PerfSnapshot, PerfSloResult } from '../types/api';

export const PerfController = {
  getPerfSnapshot(): Promise<PerfSnapshot> {
    return api.stats.getPerfSnapshot();
  },

  checkSlo(): Promise<PerfSloResult> {
    return api.stats.checkPerfSlo();
  },
};
