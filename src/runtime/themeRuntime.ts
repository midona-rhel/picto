import { appController } from '../controllers/appController';
import { settingsController } from '../controllers/settingsController';
import { registerAppSettingsReload } from './appSettingsSettle';

export type PictoTheme =
  | 'auto'
  | 'light'
  | 'lightgray'
  | 'gray'
  | 'dark'
  | 'blue'
  | 'purple'
  | 'vibrancy'
  | 'liquidglass'
  | 'mica'
  | 'acrylic';

export type PlatformFamily = 'mac' | 'windows' | 'linux';

const THEMES = new Set<PictoTheme>([
  'auto', 'light', 'lightgray', 'gray', 'dark', 'blue', 'purple',
  'vibrancy', 'liquidglass', 'mica', 'acrylic',
]);
const LIGHT_THEMES = new Set<PictoTheme>(['light', 'lightgray']);
const MAC_NATIVE = new Set<PictoTheme>(['vibrancy', 'liquidglass']);
const WINDOWS_NATIVE = new Set<PictoTheme>(['mica', 'acrylic']);

export interface ResolvedTheme {
  requested: PictoTheme;
  applied: Exclude<PictoTheme, 'auto'>;
  colorScheme: 'light' | 'dark';
}

function currentPlatform(): PlatformFamily {
  const platform = navigator.platform.toLowerCase();
  if (platform.includes('mac')) return 'mac';
  if (platform.includes('win')) return 'windows';
  return 'linux';
}

export function normalizeTheme(value: unknown): PictoTheme {
  return typeof value === 'string' && THEMES.has(value as PictoTheme)
    ? value as PictoTheme
    : 'dark';
}

export function resolveTheme(
  value: unknown,
  osDark: boolean,
  platform: PlatformFamily = currentPlatform(),
): ResolvedTheme {
  const requested = normalizeTheme(value);
  let applied: Exclude<PictoTheme, 'auto'> = requested === 'auto'
    ? (osDark ? 'dark' : 'light')
    : requested;

  if ((MAC_NATIVE.has(applied) && platform !== 'mac') ||
      (WINDOWS_NATIVE.has(applied) && platform !== 'windows')) {
    applied = 'dark';
  }

  return {
    requested,
    applied,
    colorScheme: LIGHT_THEMES.has(applied) ? 'light' : 'dark',
  };
}

export function applyTheme(value: unknown, osDark = matchMedia('(prefers-color-scheme: dark)').matches): ResolvedTheme {
  const resolved = resolveTheme(value, osDark);
  const root = document.documentElement;
  root.dataset.theme = resolved.applied;
  root.dataset.mantineColorScheme = resolved.colorScheme;
  root.style.colorScheme = resolved.colorScheme;
  return resolved;
}

let requestedTheme: PictoTheme = 'dark';
let stopRuntime: (() => void) | null = null;

function applyRequested(osDark?: boolean): void {
  applyTheme(requestedTheme, osDark);
}

export function previewTheme(value: unknown, publish = true): PictoTheme {
  requestedTheme = normalizeTheme(value);
  applyRequested();
  if (publish) void appController.publishThemePreview(requestedTheme).catch(() => {});
  return requestedTheme;
}

export function getRequestedTheme(): PictoTheme {
  return requestedTheme;
}

/** One renderer-local owner; repeated starts replace the previous subscriptions. */
export function startThemeRuntime(): () => void {
  stopRuntime?.();
  let disposed = false;
  const cleanups: Array<() => void> = [];

  const loadPersistedTheme = () => void settingsController.getSettings().then((settings) => {
    if (!disposed) previewTheme(settings.colorScheme, false);
  }).catch(() => {});
  loadPersistedTheme();
  cleanups.push(registerAppSettingsReload(loadPersistedTheme));

  void appController.subscribeThemePreview(({ theme }) => {
    if (!disposed) previewTheme(theme, false);
  }).then((cleanup) => {
    if (disposed) cleanup(); else cleanups.push(cleanup);
  });

  void appController.subscribeOsThemeChanged(({ isDark }) => {
    if (!disposed && requestedTheme === 'auto') applyRequested(isDark);
  }).then((cleanup) => {
    if (disposed) cleanup(); else cleanups.push(cleanup);
  });

  const stop = () => {
    if (disposed) return;
    disposed = true;
    cleanups.splice(0).forEach((cleanup) => cleanup());
    if (stopRuntime === stop) stopRuntime = null;
  };
  stopRuntime = stop;
  return stop;
}

export function themeNeedsNativeWindowRestart(before: unknown, after: unknown): boolean {
  const left = normalizeTheme(before);
  const right = normalizeTheme(after);
  return left !== right && (MAC_NATIVE.has(left) || MAC_NATIVE.has(right) || WINDOWS_NATIVE.has(left) || WINDOWS_NATIVE.has(right));
}
