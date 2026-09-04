import { useMemo, useSyncExternalStore } from 'react';
import { MantineProvider, type MantineProviderProps, type MantineColorSchemeManager } from '@mantine/core';
import { getThemeColorScheme, previewTheme, subscribeThemeColorScheme } from './themeRuntime';

// Picto owns the document attributes; Mantine must not restore its independent
// local-storage scheme over the library theme during its mount effects.
const runtimeOwnsRoot = () => undefined;

export function PictoThemeProvider(props: Omit<MantineProviderProps, 'forceColorScheme' | 'getRootElement' | 'defaultColorScheme' | 'colorSchemeManager'>) {
  const colorScheme = useSyncExternalStore(subscribeThemeColorScheme, getThemeColorScheme);
  // Adapt Mantine to the existing runtime, with no independent storage or
  // storage-event listener. Scheme changes are previews; Settings commits them.
  const manager = useMemo<MantineColorSchemeManager>(() => {
    let unsubscribe: (() => void) | undefined;
    return {
      get: getThemeColorScheme,
      set: (value) => { previewTheme(value); },
      clear: () => { previewTheme('auto'); },
      subscribe: (listener) => {
        unsubscribe?.();
        unsubscribe = subscribeThemeColorScheme(() => listener(getThemeColorScheme()));
      },
      unsubscribe: () => { unsubscribe?.(); unsubscribe = undefined; },
    };
  }, []);
  return <MantineProvider {...props} colorSchemeManager={manager} forceColorScheme={colorScheme} getRootElement={runtimeOwnsRoot} />;
}
