import { openSettingsWindow } from '../platform/shellApi';

export const appController = {
  openSettingsWindow(): Promise<void> {
    return openSettingsWindow();
  },

  restartMainWindow(): Promise<void> {
    return (window as any).picto?.api?.restartMainWindow?.() ?? Promise.resolve();
  },
};
