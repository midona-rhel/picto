import { createContext, useContext } from 'react';
import type { DisplayOptions } from '../../hooks/useScopedGridPreferences';

interface ScopedDisplayContextValue {
  displayOptions: DisplayOptions;
  onDisplayOptionChange: <K extends keyof DisplayOptions>(key: K, value: DisplayOptions[K]) => void;
}

const ScopedDisplayContext = createContext<ScopedDisplayContextValue | null>(null);

export const ScopedDisplayProvider = ScopedDisplayContext.Provider;

/**
 * Returns scoped display options if inside a ScopedDisplayProvider,
 * otherwise returns null (caller should fall back to global settings).
 */
export function useScopedDisplay(): ScopedDisplayContextValue | null {
  return useContext(ScopedDisplayContext);
}
