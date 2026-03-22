/**
 * Settings store backed by #desktop/api.
 *
 * Replaces the old useSettings hook (invoke get_settings / save_settings).
 * Uses onKeyChange() for reactive push updates — no polling needed.
 */

import { create } from 'zustand';
import { load, type Store } from '#desktop/api';

export type ReverseSearchEngine = 'tineye' | 'saucenao' | 'yandex' | 'sogou' | 'bing';
export type Theme = 'auto' | 'dark' | 'blue' | 'purple' | 'gray' | 'light' | 'lightgray';

export interface AppSettings {
  gridTargetSize: number;
  gridViewMode: 'waterfall' | 'justified' | 'grid';
  inspectorWidth: number;
  colorScheme: 'dark' | 'light';
  theme: Theme;
  gridSortField: 'date_added' | 'size' | 'rating';
  gridSortOrder: 'asc' | 'desc';
  showTileName: boolean;
  showResolution: boolean;
  showExtension: boolean;
  showExtensionLabel: boolean;
  showSubfolders: boolean;
  showSidebar: boolean;
  showInspector: boolean;
  thumbnailFitMode: 'cover' | 'contain';
  enabledSearchEngines: ReverseSearchEngine[];
  videoAutoPlay: boolean;
  videoLoop: boolean;
  videoMuted: boolean;
  videoVolume: number;
  videoPlaybackRate: number;
  grayscalePreview: boolean;
  showNavigator: boolean;
  hideTagNamespace: boolean;
  stripDefaultFitMode: 'horizontal' | 'vertical';
  /** Strip view keyboard scroll speed: pixels per frame at max velocity. */
  stripScrollSpeed: number;
  /** Enable keyboard scrolling in strip view. */
  stripScrollEnabled: boolean;
  /** Enable smooth zoom transitions in media view. */
  smoothZoomEnabled: boolean;
}

/** Derive Mantine color scheme from a Theme value. */
export function themeToColorScheme(theme: Theme): 'auto' | 'dark' | 'light' {
  if (theme === 'auto') return 'auto';
  if (theme === 'light' || theme === 'lightgray') return 'light';
  return 'dark';
}

const DEFAULTS: AppSettings = {
  gridTargetSize: 250,
  gridViewMode: 'waterfall',
  inspectorWidth: 250,
  colorScheme: 'dark',
  theme: 'dark',
  gridSortField: 'date_added',
  gridSortOrder: 'asc',
  showTileName: true,
  showResolution: true,
  showExtension: true,
  showExtensionLabel: true,
  showSubfolders: true,
  showSidebar: true,
  showInspector: true,
  thumbnailFitMode: 'cover',
  enabledSearchEngines: ['tineye', 'saucenao', 'yandex', 'sogou', 'bing'],
  videoAutoPlay: true,
  videoLoop: true,
  videoMuted: true,
  videoVolume: 0.9,
  videoPlaybackRate: 1.0,
  grayscalePreview: false,
  showNavigator: true,
  hideTagNamespace: false,
  stripDefaultFitMode: 'horizontal',
  stripScrollSpeed: 108,
  stripScrollEnabled: false,
  smoothZoomEnabled: false,
};

interface SettingsState {
  settings: AppSettings;
  loaded: boolean;
  /** Update a single key and persist to the store. */
  updateSetting: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void;
  /** Update multiple keys atomically (single state update + single save). */
  updateSettings: (updates: Partial<AppSettings>) => void;
}

// Module-level store instance so we don't create multiple
let storeInstance: Store | null = null;
let storeReady = false;

export const useSettingsStore = create<SettingsState>((set, _get) => ({
  settings: DEFAULTS,
  loaded: false,

  updateSetting: (key, value) => {
    set((state) => ({
      settings: { ...state.settings, [key]: value },
    }));
    // Persist async — fire and forget
    if (storeInstance && storeReady) {
      void storeInstance.set(key, value).then(() => storeInstance!.save());
    }
  },

  updateSettings: (updates) => {
    set((state) => ({
      settings: { ...state.settings, ...updates },
    }));
    if (storeInstance && storeReady) {
      const entries = Object.entries(updates);
      void Promise.all(entries.map(([k, v]) => storeInstance!.set(k, v)))
        .then(() => storeInstance!.save());
    }
  },
}));

/**
 * Initialize the plugin store and hydrate Zustand from disk.
 * Call once from App.tsx on mount.
 */
export async function initSettingsStore(): Promise<void> {
  try {
    storeInstance = await load('settings.json', { autoSave: false });

    // Hydrate from disk
    const hydrated: Partial<AppSettings> = {};
    for (const key of Object.keys(DEFAULTS) as (keyof AppSettings)[]) {
      const val = await storeInstance.get(key);
      if (val !== null && val !== undefined) {
        (hydrated as Record<string, unknown>)[key] = val;
      }
    }

    useSettingsStore.setState({
      settings: { ...DEFAULTS, ...hydrated },
      loaded: true,
    });
    storeReady = true;

    // Subscribe to reactive changes (e.g. from other windows)
    for (const key of Object.keys(DEFAULTS) as (keyof AppSettings)[]) {
      void storeInstance.onKeyChange(key, (val) => {
        if (val !== null && val !== undefined) {
          const current = useSettingsStore.getState().settings[key];
          console.log('[settings] onKeyChange:', key, '| new:', val, '| current:', current);
          useSettingsStore.setState((state) => ({
            settings: { ...state.settings, [key]: val },
          }));
        }
      });
    }
  } catch (err) {
    console.error('Failed to init settings store:', err);
    // Fall back to defaults
    useSettingsStore.setState({ loaded: true });
  }
}
