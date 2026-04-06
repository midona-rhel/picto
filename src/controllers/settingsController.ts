import {
  getSettings,
  getViewPrefs,
  saveSettings,
  setViewPrefs,
  setZoomFactor,
} from '../platform/settingsApi';
import type { AppSettings, ViewPrefsDto, ViewPrefsPatch } from '../platform/settingsApi';

export type { AppSettings, ViewPrefsDto, ViewPrefsPatch } from '../platform/settingsApi';

export const settingsController = {
  getSettings(): Promise<AppSettings> {
    return getSettings();
  },

  saveSettings(settings: Partial<AppSettings>): Promise<void> {
    return saveSettings(settings);
  },

  setZoomFactor(factor: number): Promise<void> {
    return setZoomFactor(factor);
  },

  getViewPrefs(scopeKey: string): Promise<ViewPrefsDto> {
    return getViewPrefs(scopeKey);
  },

  setViewPrefs(scopeKey: string, patch: ViewPrefsPatch): Promise<ViewPrefsDto> {
    return setViewPrefs(scopeKey, patch);
  },
};
