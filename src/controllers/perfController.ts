import { api } from '#desktop/api';
import type { PerfSloResult } from '../shared/types/api';

export const PerfController = {
  checkSlo(): Promise<PerfSloResult> {
    return api.stats.checkPerfSlo();
  },
};
