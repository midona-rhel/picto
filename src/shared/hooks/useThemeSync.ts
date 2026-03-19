import { useEffect } from 'react';
import { initSettingsStore, themeToColorScheme, useSettingsStore } from '../../state/settingsStore';

/**
 * Unified theme sync hook — initializes settings store and keeps
 * the DOM theme attribute in sync with user settings.
 *
 * Mantine color scheme is controlled via forceColorScheme prop on
 * MantineProvider (not setColorScheme) to avoid cross-window
 * localStorage storage-event conflicts.
 */
export function useThemeSync(): void {
  const { settings, loaded: settingsLoaded } = useSettingsStore();

  useEffect(() => {
    void initSettingsStore();
  }, []);

  useEffect(() => {
    if (!settingsLoaded) return;
    const theme = settings.theme ?? (settings.colorScheme === 'light' ? 'light' : 'dark');
    document.documentElement.dataset.theme = theme === 'auto' ? '' : theme;
  }, [settingsLoaded, settings.theme, settings.colorScheme]);
}

/**
 * Derive the Mantine color scheme from the settings store.
 * Used as forceColorScheme prop on MantineProvider.
 */
export function useDerivedColorScheme(): 'light' | 'dark' {
  const { settings } = useSettingsStore();
  const theme = settings.theme ?? 'dark';
  const scheme = themeToColorScheme(theme);
  return scheme === 'auto' ? 'dark' : scheme;
}
