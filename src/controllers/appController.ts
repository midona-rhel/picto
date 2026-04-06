import * as api from '../platform/api';

export const appController = {
  openSettingsWindow(): Promise<void> {
    return api.openSettingsWindow();
  },
};
