import { listen } from '../platform/ipc';
import { openSettingsWindow } from '../platform/shellApi';

export const appController = {
  openSettingsWindow(): Promise<void> {
    return openSettingsWindow();
  },

  subscribeOsThemeChanged(onChange: (payload: { isDark: boolean }) => void): Promise<() => void> {
    return listen<{ isDark: boolean }>('picto:os-theme-changed', (event) => {
      onChange(event.payload);
    });
  },

  subscribeMenuNavigate(onNavigate: (destination: string) => void): Promise<() => void> {
    return listen<string>('menu:navigate', (event) => {
      onNavigate(event.payload);
    });
  },

  restartMainWindow(): Promise<void> {
    return (window as any).picto?.api?.restartMainWindow?.() ?? Promise.resolve();
  },
};
