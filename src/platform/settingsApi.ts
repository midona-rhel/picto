import { invoke } from './ipc';

export interface ViewPrefsDto {
  scope_key: string;
  sort_field: string | null;
  sort_order: string | null;
  view_mode: string | null;
  target_size: number | null;
  show_name: boolean | null;
  show_resolution: boolean | null;
  show_extension: boolean | null;
  show_label: boolean | null;
  thumbnail_fit: string | null;
}

export interface ViewPrefsPatch {
  sort_field?: string | null;
  sort_order?: string | null;
  view_mode?: string | null;
  target_size?: number | null;
  show_name?: boolean | null;
  show_resolution?: boolean | null;
  show_extension?: boolean | null;
  show_label?: boolean | null;
  thumbnail_fit?: string | null;
  show_subfolders?: boolean | null;
}

export function setZoomFactor(factor: number): Promise<void> {
  return invoke<void>('set_zoom_factor', { factor });
}

export function getViewPrefs(scopeKey: string): Promise<ViewPrefsDto> {
  return invoke<ViewPrefsDto>('get_view_prefs', { scope_key: scopeKey });
}

export function setViewPrefs(scopeKey: string, patch: ViewPrefsPatch): Promise<ViewPrefsDto> {
  return invoke<ViewPrefsDto>('set_view_prefs', { scope_key: scopeKey, patch });
}

export interface AppSettings {
  gridTargetSize: number;
  gridViewMode: string;
  inspectorWidth: number;
  colorScheme: string;
  gridSortField: string;
  gridSortOrder: string;
  zoomFactor: number | null;
  showTreeGuides: boolean;
  aiTaggerWd14Enabled: boolean;
  aiTaggerE621Enabled: boolean;
  aiTaggerEva02Enabled: boolean;
  aiTaggerAutoOnImport: boolean;
  aiTaggerWriteRating: boolean;
  aiThresholdGeneral: number;
  aiThresholdCharacter: number;
  aiThresholdCopyright: number;
  aiThresholdArtist: number;
  aiThresholdSpecies: number;
  aiThresholdRating: number;
  [key: string]: unknown;
}

export function getSettings(): Promise<AppSettings> {
  return invoke<AppSettings>('get_settings', {});
}

export function saveSettings(settings: Partial<AppSettings>): Promise<void> {
  return getSettings().then((current) =>
    invoke<void>('save_settings', { ...current, ...settings }),
  );
}
