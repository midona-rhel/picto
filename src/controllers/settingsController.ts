import {
  getSettings,
  getSettingsSnapshot,
  getViewPrefs,
  getViewPrefsSnapshot,
  patchSettings,
  replaceSettings,
  resetViewPrefs,
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
import { announceUndoableMutation } from '../runtime/historyRuntime';

export type { AppSettings, ViewPrefsDto, ViewPrefsPatch } from '../platform/settingsApi';

export const settingsController = {
  getSettings(): Promise<AppSettings> {
    return getSettings();
  },

  getSettingsSnapshot,

  async saveSettings(settings: Partial<AppSettings>): Promise<MutationReceipt> {
    const receipt = await saveSettings(settings);
    await announceUndoableMutation('settings.patch');
    return receipt;
  },

  async patchSettings(settings: Partial<AppSettings>): Promise<MutationReceipt> {
    const receipt = await patchSettings(settings);
    await announceUndoableMutation('settings.patch');
    return receipt;
  },

  async replaceSettings(settings: AppSettings): Promise<MutationReceipt> {
    const receipt = await replaceSettings(settings);
    await announceUndoableMutation('settings.replace');
    return receipt;
  },

  getViewPrefs(scopeKey: string): Promise<ViewPrefsDto> {
    return getViewPrefs(scopeKey);
  },

  getViewPrefsSnapshot,

  viewPrefsToPatch,

  async setViewPrefs(scopeKey: string, patch: ViewPrefsPatch): Promise<MutationReceipt> {
    return setViewPrefs(scopeKey, patch);
  },

  resetViewPrefs(): Promise<MutationReceipt> {
    return resetViewPrefs();
  },
};
