import { invoke } from './ipc';
import type { MutationReceipt } from '../shared/types/generated/application/MutationReceipt';
import type { SettingsSnapshot } from '../shared/types/generated/application/SettingsSnapshot';
import type { GridSpacing } from '../shared/types/grid';

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
  show_item_count: boolean | null;
  thumbnail_fit: string | null;
  show_subfolders: boolean | null;
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
  show_item_count?: boolean | null;
  thumbnail_fit?: string | null;
  show_subfolders?: boolean | null;
}

export interface AppSettings {
  gridTargetSize: number;
  gridViewMode: string;
  gridSpacing: GridSpacing;
  inspectorWidth: number;
  colorScheme: string;
  gridSortField: string;
  gridSortOrder: string;
  zoomFactor: number | null;
  showTreeGuides: boolean;
  showTagGroups: boolean;
  starredTags: string[];
  sidebarQuickAccess: string[];
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

const APP_SETTINGS_DEFAULTS: AppSettings = {
  gridTargetSize: 250,
  gridViewMode: 'waterfall',
  gridSpacing: 'wide',
  inspectorWidth: 280,
  colorScheme: 'dark',
  gridSortField: 'imported_at',
  gridSortOrder: 'ascending',
  zoomFactor: null,
  showTreeGuides: true,
  showTagGroups: true,
  starredTags: [],
  sidebarQuickAccess: [],
  aiTaggerWd14Enabled: false,
  aiTaggerE621Enabled: false,
  aiTaggerEva02Enabled: false,
  aiTaggerAutoOnImport: false,
  aiTaggerWriteRating: true,
  aiThresholdGeneral: 0.35,
  aiThresholdCharacter: 0.85,
  aiThresholdCopyright: 0.85,
  aiThresholdArtist: 0.85,
  aiThresholdSpecies: 0.35,
  aiThresholdRating: 0.5,
};

type JsonObject = Record<string, unknown>;

function objectValue(value: unknown, label: string): JsonObject {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error(`${label} must be an object.`);
  }
  return value as JsonObject;
}

function revisionValue(value: unknown): number {
  if (!Number.isSafeInteger(value) || (value as number) < 0) {
    throw new Error('Settings revision must be a non-negative integer.');
  }
  return value as number;
}

function numberValue(value: unknown, key: string): number {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    throw new Error(`Settings field "${key}" must be a finite number.`);
  }
  return value;
}

function stringValue(value: unknown, key: string): string {
  if (typeof value !== 'string') throw new Error(`Settings field "${key}" must be a string.`);
  return value;
}

function gridSpacingValue(value: unknown): GridSpacing {
  if (value !== 'wide' && value !== 'tight') {
    throw new Error('Settings field "gridSpacing" must be "wide" or "tight".');
  }
  return value;
}

function booleanValue(value: unknown, key: string): boolean {
  if (typeof value !== 'boolean') throw new Error(`Settings field "${key}" must be a boolean.`);
  return value;
}

function nullableNumberValue(value: unknown, key: string): number | null {
  if (value === null) return null;
  return numberValue(value, key);
}

function stringArrayValue(value: unknown, key: string): string[] {
  if (!Array.isArray(value) || value.some((item) => typeof item !== 'string')) {
    throw new Error(`Settings field "${key}" must be an array of strings.`);
  }
  return [...new Set(value.map((item) => item.trim()).filter(Boolean))];
}

function storedOrDefault(source: JsonObject, key: string, fallback: unknown): unknown {
  return Object.prototype.hasOwnProperty.call(source, key) ? source[key] : fallback;
}

function parseAppSettings(snapshot: SettingsSnapshot): { value: AppSettings; revision: number } {
  const source = objectValue(snapshot.value, 'Application settings');
  const value: AppSettings = {
    ...source,
    gridTargetSize: numberValue(storedOrDefault(source, 'gridTargetSize', APP_SETTINGS_DEFAULTS.gridTargetSize), 'gridTargetSize'),
    gridViewMode: stringValue(storedOrDefault(source, 'gridViewMode', APP_SETTINGS_DEFAULTS.gridViewMode), 'gridViewMode'),
    gridSpacing: gridSpacingValue(storedOrDefault(source, 'gridSpacing', APP_SETTINGS_DEFAULTS.gridSpacing)),
    inspectorWidth: numberValue(storedOrDefault(source, 'inspectorWidth', APP_SETTINGS_DEFAULTS.inspectorWidth), 'inspectorWidth'),
    colorScheme: stringValue(storedOrDefault(source, 'colorScheme', APP_SETTINGS_DEFAULTS.colorScheme), 'colorScheme'),
    gridSortField: stringValue(storedOrDefault(source, 'gridSortField', APP_SETTINGS_DEFAULTS.gridSortField), 'gridSortField'),
    gridSortOrder: stringValue(storedOrDefault(source, 'gridSortOrder', APP_SETTINGS_DEFAULTS.gridSortOrder), 'gridSortOrder'),
    zoomFactor: nullableNumberValue(storedOrDefault(source, 'zoomFactor', APP_SETTINGS_DEFAULTS.zoomFactor), 'zoomFactor'),
    showTreeGuides: booleanValue(storedOrDefault(source, 'showTreeGuides', APP_SETTINGS_DEFAULTS.showTreeGuides), 'showTreeGuides'),
    showTagGroups: booleanValue(storedOrDefault(source, 'showTagGroups', APP_SETTINGS_DEFAULTS.showTagGroups), 'showTagGroups'),
    starredTags: stringArrayValue(storedOrDefault(source, 'starredTags', APP_SETTINGS_DEFAULTS.starredTags), 'starredTags'),
    sidebarQuickAccess: stringArrayValue(storedOrDefault(source, 'sidebarQuickAccess', APP_SETTINGS_DEFAULTS.sidebarQuickAccess), 'sidebarQuickAccess'),
    aiTaggerWd14Enabled: booleanValue(storedOrDefault(source, 'aiTaggerWd14Enabled', APP_SETTINGS_DEFAULTS.aiTaggerWd14Enabled), 'aiTaggerWd14Enabled'),
    aiTaggerE621Enabled: booleanValue(storedOrDefault(source, 'aiTaggerE621Enabled', APP_SETTINGS_DEFAULTS.aiTaggerE621Enabled), 'aiTaggerE621Enabled'),
    aiTaggerEva02Enabled: booleanValue(storedOrDefault(source, 'aiTaggerEva02Enabled', APP_SETTINGS_DEFAULTS.aiTaggerEva02Enabled), 'aiTaggerEva02Enabled'),
    aiTaggerAutoOnImport: booleanValue(storedOrDefault(source, 'aiTaggerAutoOnImport', APP_SETTINGS_DEFAULTS.aiTaggerAutoOnImport), 'aiTaggerAutoOnImport'),
    aiTaggerWriteRating: booleanValue(storedOrDefault(source, 'aiTaggerWriteRating', APP_SETTINGS_DEFAULTS.aiTaggerWriteRating), 'aiTaggerWriteRating'),
    aiThresholdGeneral: numberValue(storedOrDefault(source, 'aiThresholdGeneral', APP_SETTINGS_DEFAULTS.aiThresholdGeneral), 'aiThresholdGeneral'),
    aiThresholdCharacter: numberValue(storedOrDefault(source, 'aiThresholdCharacter', APP_SETTINGS_DEFAULTS.aiThresholdCharacter), 'aiThresholdCharacter'),
    aiThresholdCopyright: numberValue(storedOrDefault(source, 'aiThresholdCopyright', APP_SETTINGS_DEFAULTS.aiThresholdCopyright), 'aiThresholdCopyright'),
    aiThresholdArtist: numberValue(storedOrDefault(source, 'aiThresholdArtist', APP_SETTINGS_DEFAULTS.aiThresholdArtist), 'aiThresholdArtist'),
    aiThresholdSpecies: numberValue(storedOrDefault(source, 'aiThresholdSpecies', APP_SETTINGS_DEFAULTS.aiThresholdSpecies), 'aiThresholdSpecies'),
    aiThresholdRating: numberValue(storedOrDefault(source, 'aiThresholdRating', APP_SETTINGS_DEFAULTS.aiThresholdRating), 'aiThresholdRating'),
  };

  return { value, revision: revisionValue(snapshot.revision) };
}

function nullableField(source: JsonObject, key: string, kind: 'number'): number | null;
function nullableField(source: JsonObject, key: string, kind: 'string'): string | null;
function nullableField(source: JsonObject, key: string, kind: 'boolean'): boolean | null;
function nullableField(source: JsonObject, key: string, kind: 'number' | 'string' | 'boolean'):
  number | string | boolean | null {
  const value = source[key];
  if (value === undefined || value === null) return null;
  if (kind === 'number') return numberValue(value, key);
  if (kind === 'string') return stringValue(value, key);
  return booleanValue(value, key);
}

function parseViewPrefs(snapshot: SettingsSnapshot, scopeKey: string): { value: ViewPrefsDto; revision: number } {
  const source = objectValue(snapshot.value, 'View preferences');
  return {
    value: {
      scope_key: scopeKey,
      sort_field: nullableField(source, 'sort_field', 'string'),
      sort_order: nullableField(source, 'sort_order', 'string'),
      view_mode: nullableField(source, 'view_mode', 'string'),
      target_size: nullableField(source, 'target_size', 'number'),
      show_name: nullableField(source, 'show_name', 'boolean'),
      show_resolution: nullableField(source, 'show_resolution', 'boolean'),
      show_extension: nullableField(source, 'show_extension', 'boolean'),
      show_label: nullableField(source, 'show_label', 'boolean'),
      show_item_count: nullableField(source, 'show_item_count', 'boolean'),
      thumbnail_fit: nullableField(source, 'thumbnail_fit', 'string'),
      show_subfolders: nullableField(source, 'show_subfolders', 'boolean'),
    },
    revision: revisionValue(snapshot.revision),
  };
}

export async function getSettingsSnapshot(): Promise<{ value: AppSettings; revision: number }> {
  return parseAppSettings(await invoke<SettingsSnapshot>('settings.get', {}));
}

export async function getSettings(): Promise<AppSettings> {
  return (await getSettingsSnapshot()).value;
}

export function patchSettings(settings: Partial<AppSettings>): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('settings.patch', { value: settings });
}

export function replaceSettings(settings: AppSettings): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('settings.replace', { value: settings });
}

// Existing callers use this as the incremental settings write path.
export function saveSettings(settings: Partial<AppSettings>): Promise<MutationReceipt> {
  return patchSettings(settings);
}

export async function getViewPrefsSnapshot(scopeKey: string): Promise<{ value: ViewPrefsDto; revision: number }> {
  return parseViewPrefs(
    await invoke<SettingsSnapshot>('settings.view.get', { scope: scopeKey }),
    scopeKey,
  );
}

export async function getViewPrefs(scopeKey: string): Promise<ViewPrefsDto> {
  return (await getViewPrefsSnapshot(scopeKey)).value;
}

export function viewPrefsToPatch(prefs: ViewPrefsDto): ViewPrefsPatch {
  return {
    sort_field: prefs.sort_field,
    sort_order: prefs.sort_order,
    view_mode: prefs.view_mode,
    target_size: prefs.target_size,
    show_name: prefs.show_name,
    show_resolution: prefs.show_resolution,
    show_extension: prefs.show_extension,
    show_label: prefs.show_label,
    show_item_count: prefs.show_item_count,
    thumbnail_fit: prefs.thumbnail_fit,
    show_subfolders: prefs.show_subfolders,
  };
}

export function setViewPrefs(scopeKey: string, patch: ViewPrefsPatch): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('settings.view.patch', { scope: scopeKey, value: patch });
}
