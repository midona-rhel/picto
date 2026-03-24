import { api } from '#desktop/api';
import type { AppSettings, ViewPrefsDto, ViewPrefsPatch } from '../shared/types/api';

export const settingsController = {
  get(): Promise<AppSettings> {
    return api.settings.get();
  },

  save(settings: Partial<AppSettings>) {
    return api.settings.save(settings);
  },

  setZoomFactor(factor: number) {
    return api.settings.setZoomFactor(factor);
  },

  getViewPrefs(scopeKey?: string): Promise<ViewPrefsDto | null> {
    return api.settings.getViewPrefs(scopeKey);
  },

  setViewPrefs(scopeKey: string | undefined, patch: ViewPrefsPatch): Promise<ViewPrefsDto> {
    return api.settings.setViewPrefs(scopeKey, patch);
  },
};
