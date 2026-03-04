import { api } from '#desktop/api';

// Re-export types from central api types for backwards compatibility.
export type { ViewPrefsDto, ViewPrefsPatch } from '../types/api';
import type { ViewPrefsDto, ViewPrefsPatch } from '../types/api';

/**
 * ViewPrefsController — orchestration facade for per-scope grid view prefs.
 */
export const ViewPrefsController = {
  get(scopeKey: string): Promise<ViewPrefsDto | null> {
    return api.settings.getViewPrefs(scopeKey);
  },

  set(scopeKey: string, patch: ViewPrefsPatch): Promise<ViewPrefsDto> {
    return api.settings.setViewPrefs(scopeKey, patch);
  },
};
