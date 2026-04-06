import {
  openDetailWindow,
  openExternalUrl,
  openSettingsWindow,
} from '../platform/shellApi';

export const shellController = {
  openExternalUrl(url: string): Promise<void> {
    return openExternalUrl(url);
  },

  openSettingsWindow(): Promise<void> {
    return openSettingsWindow();
  },

  openDetailWindow(input: {
    hash: string;
    width?: number | null;
    height?: number | null;
  }): Promise<void> {
    return openDetailWindow(input);
  },
};
