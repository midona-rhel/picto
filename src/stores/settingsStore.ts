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
  propertiesPanelWidth: number;
  colorScheme: 'dark' | 'light';
  theme: Theme;
  gridSortField: 'imported_at' | 'size' | 'rating' | 'view_count';
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
  showMinimap: boolean;
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
  propertiesPanelWidth: 250,
  colorScheme: 'dark',
  theme: 'dark',
  gridSortField: 'imported_at',
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
  showMinimap: true,
};

interface SettingsState {
  settings: AppSettings;
  loaded: boolean;
  /** Update a single key and persist to the store. */
  updateSetting: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void;
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
