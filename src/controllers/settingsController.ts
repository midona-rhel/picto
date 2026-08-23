import {
  getSettings,
  getSettingsSnapshot,
  getViewPrefs,
  getViewPrefsSnapshot,
  patchSettings,
  replaceSettings,
  saveSettings,
  setViewPrefs,
  viewPrefsToPatch,
} from '../platform/settingsApi';
import type {
  AppSettings,
  ViewPrefsDto,
  ViewPrefsPatch,
} from '../platform/settingsApi';
import type { MutationReceipt } from '../shared/types/generated/application/MutationReceipt';

export type { AppSettings, ViewPrefsDto, ViewPrefsPatch } from '../platform/settingsApi';

export const settingsController = {
  getSettings(): Promise<AppSettings> {
    return getSettings();
  },

  getSettingsSnapshot,

  saveSettings(settings: Partial<AppSettings>): Promise<MutationReceipt> {
    return saveSettings(settings);
  },

  patchSettings(settings: Partial<AppSettings>): Promise<MutationReceipt> {
    return patchSettings(settings);
  },

  replaceSettings(settings: AppSettings): Promise<MutationReceipt> {
    return replaceSettings(settings);
  },

  getViewPrefs(scopeKey: string): Promise<ViewPrefsDto> {
    return getViewPrefs(scopeKey);
  },

  getViewPrefsSnapshot,

  viewPrefsToPatch,

  setViewPrefs(scopeKey: string, patch: ViewPrefsPatch): Promise<MutationReceipt> {
    return setViewPrefs(scopeKey, patch);
  },
};
