/**
 * Application settings — persisted to backend, hydrated on startup.
 *
 * Grid display preferences, theme, and behavior toggles.
 */

import { atom } from 'jotai';

export interface AppSettings {
  gridViewMode: 'waterfall' | 'grid' | 'justified';
  gridTargetSize: number;
  gridSortField: string;
  gridSortOrder: 'asc' | 'desc';
  theme: string;
  showTileName: boolean;
  showResolution: boolean;
  showExtension: boolean;
  showExtensionLabel: boolean;
  showSubfolders: boolean;
  thumbnailFitMode: 'cover' | 'contain';
  videoAutoplay: boolean;
  videoVolume: number;
  videoMuted: boolean;
  scrollSpeed: number;
}

const defaultSettings: AppSettings = {
  gridViewMode: 'waterfall',
  gridTargetSize: 220,
  gridSortField: 'date_added',
  gridSortOrder: 'desc',
  theme: 'dark',
  showTileName: false,
  showResolution: false,
  showExtension: false,
  showExtensionLabel: false,
  showSubfolders: true,
  thumbnailFitMode: 'cover',
  videoAutoplay: true,
  videoVolume: 0.5,
  videoMuted: false,
  scrollSpeed: 1.0,
};

export const settingsAtom = atom<AppSettings>(defaultSettings);

/** Update a single setting. */
export const updateSettingAtom = atom(
  null,
  (get, set, update: Partial<AppSettings>) => {
    set(settingsAtom, { ...get(settingsAtom), ...update });
  },
);
