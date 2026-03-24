import { api } from '#desktop/api';
import { registerUndoAction } from '../shared/controllers/undoRedoController';
import type {
  DuplicatePairsResponse,
  DuplicateSettings,
  ResolveDuplicateAction,
  ScanDuplicatesResult,
  SmartMergeResult,
} from '../shared/types/api';

export const duplicatesController = {
  getCount(): Promise<{ count: number }> {
    return api.duplicates.getCount();
  },

  getPairs(cursor?: string | null, limit?: number, status?: string): Promise<DuplicatePairsResponse> {
    return api.duplicates.getPairs(cursor, limit, status);
  },

  resolvePair(action: ResolveDuplicateAction, hashA: string, hashB: string): Promise<SmartMergeResult | Record<string, string>> {
    return api.duplicates.resolvePair(action, hashA, hashB);
  },

  scan(): Promise<ScanDuplicatesResult> {
    return api.duplicates.scan();
  },

  getSettings(): Promise<DuplicateSettings> {
    return api.duplicates.getSettings();
  },

  async updateSettings(next: Partial<DuplicateSettings>, previous: DuplicateSettings): Promise<{ ok: boolean }> {
    const result = await api.duplicates.updateSettings(next);
    const nextSnapshot = { ...next };
    const prevSnapshot = { ...previous };
    registerUndoAction({
      label: 'Update duplicate settings',
      backward: async () => { await api.duplicates.updateSettings(prevSnapshot); },
      forward: async () => { await api.duplicates.updateSettings(nextSnapshot); },
    });
    return result;
  },
};
