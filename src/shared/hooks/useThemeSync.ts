import { useEffect, useRef } from 'react';
import { useMantineColorScheme } from '@mantine/core';
import { initSettingsStore, themeToColorScheme, useSettingsStore } from '../../state/settingsStore';

/**
 * Unified theme sync hook — initializes settings store and keeps
 * Mantine color scheme + DOM theme attribute in sync with user settings.
 *
 * Replaces duplicated theme effects in useAppBootstrap and all entrypoint windows.
 */
export function useThemeSync(): void {
  const { settings, loaded: settingsLoaded } = useSettingsStore();
  const { setColorScheme, colorScheme } = useMantineColorScheme();

  // Ref avoids setColorScheme/colorScheme as effect deps (they're stable but not
  // declared so by Mantine, which would force an exhaustive-deps suppression).
  const setColorSchemeRef = useRef(setColorScheme);
  setColorSchemeRef.current = setColorScheme;
  const colorSchemeRef = useRef(colorScheme);
  colorSchemeRef.current = colorScheme;

  useEffect(() => {
    void initSettingsStore();
  }, []);

  useEffect(() => {
    if (!settingsLoaded) return;
    const theme = settings.theme ?? (settings.colorScheme === 'light' ? 'light' : 'dark');
    const scheme = themeToColorScheme(theme);
    if (scheme !== colorSchemeRef.current) setColorSchemeRef.current(scheme);
    document.documentElement.dataset.theme = theme === 'auto' ? '' : theme;
  }, [settingsLoaded, settings.theme, settings.colorScheme]);
}
