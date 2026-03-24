import { useEffect, useState } from 'react';
import { initSettingsStore, themeToColorScheme, useSettingsStore } from '../../state-legacy/settingsStore';

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
    localStorage.setItem('picto-theme', theme);
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

  // Listen for OS preference changes when set to auto
  const [osPrefersDark, setOsPrefersDark] = useState(
    () => !window.matchMedia('(prefers-color-scheme: light)').matches,
  );
  useEffect(() => {
    const mq = window.matchMedia('(prefers-color-scheme: light)');
    const handler = (e: MediaQueryListEvent) => setOsPrefersDark(!e.matches);
    mq.addEventListener('change', handler);
    return () => mq.removeEventListener('change', handler);
  }, []);

  if (scheme === 'auto') {
    return osPrefersDark ? 'dark' : 'light';
  }
  return scheme;
}
