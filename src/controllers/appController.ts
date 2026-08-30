import { listen } from '../platform/ipc';
import { getApplicationMenuShortcutBindings } from '../shared/lib/shortcuts';
import { openSettingsWindow } from '../platform/shellApi';

export const appController = {
  syncApplicationMenuShortcuts(): Promise<unknown> {
    return (window as any).picto?.api?.setApplicationMenuShortcuts?.(
      getApplicationMenuShortcutBindings(),
    ) ?? Promise.resolve();
  },
  openSettingsWindow(panel?: string): Promise<void> {
    return openSettingsWindow(panel);
  },

  subscribeOsThemeChanged(onChange: (payload: { isDark: boolean }) => void): Promise<() => void> {
    return listen<{ isDark: boolean }>('picto:os-theme-changed', (event) => {
      onChange(event.payload);
    });
  },

  subscribeThemePreview(onChange: (payload: { theme: string }) => void): Promise<() => void> {
    return listen<{ theme: string }>('picto:theme-preview', (event) => onChange(event.payload));
  },

  publishThemePreview(theme: string): Promise<void> {
    return (window as any).picto?.events?.emit?.('picto:theme-preview', { theme }) ?? Promise.resolve();
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
