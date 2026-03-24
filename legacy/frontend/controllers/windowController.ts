import { openExternalUrl, openSettingsWindow, openSubscriptionsWindow, openLibraryManager } from '../platform/api';

export const windowController = {
  openExternal(url: string): Promise<void> {
    return openExternalUrl(url);
  },

  openSettings(): Promise<void> {
    return openSettingsWindow();
  },

  openSubscriptions(): Promise<void> {
    return openSubscriptionsWindow();
  },

  openLibraryManager(): Promise<void> {
    return openLibraryManager();
  },
};
