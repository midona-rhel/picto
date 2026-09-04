import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  getSettings: vi.fn(),
  subscribeThemePreview: vi.fn(),
  subscribeOsThemeChanged: vi.fn(),
  publishThemePreview: vi.fn(),
}));

vi.mock('../controllers/settingsController', () => ({
  settingsController: { getSettings: mocks.getSettings },
}));
vi.mock('../controllers/appController', () => ({
  appController: {
    subscribeThemePreview: mocks.subscribeThemePreview,
    subscribeOsThemeChanged: mocks.subscribeOsThemeChanged,
    publishThemePreview: mocks.publishThemePreview,
  },
}));

import {
  applyTheme,
  previewTheme,
  resolveTheme,
  startThemeRuntime,
  themeNeedsNativeWindowRestart,
} from './themeRuntime';
import { publishPlatform } from '../shared/lib/platform';

beforeEach(() => {
  vi.clearAllMocks();
  delete (window as any).picto;
  mocks.getSettings.mockResolvedValue({ colorScheme: 'dark' });
  mocks.subscribeThemePreview.mockResolvedValue(() => {});
  mocks.subscribeOsThemeChanged.mockResolvedValue(() => {});
  mocks.publishThemePreview.mockResolvedValue(undefined);
});

describe('themeRuntime', () => {
  it('keeps the preloaded library theme while canonical settings revalidate', async () => {
    (window as any).picto = { startupTheme: { requested: 'lightgray' } };
    mocks.getSettings.mockResolvedValue({ colorScheme: 'lightgray' });
    localStorage.setItem('picto-theme', 'purple');
    localStorage.setItem('mantine-color-scheme-value', 'dark');
    localStorage.setItem('picto:locale', 'de');
    document.documentElement.style.backgroundColor = '#e3e4e6';
    const stop = startThemeRuntime();
    expect(localStorage.getItem('picto-theme')).toBeNull();
    expect(localStorage.getItem('mantine-color-scheme-value')).toBeNull();
    expect(localStorage.getItem('picto:locale')).toBe('de');
    expect(document.documentElement.dataset.theme).toBe('lightgray');
    expect(document.documentElement.dataset.mantineColorScheme).toBe('light');
    expect(document.documentElement.style.backgroundColor).toBe('');
    await Promise.resolve();
    expect(document.documentElement.dataset.theme).toBe('lightgray');
    stop();
  });
  it('does not let a delayed startup read undo a newer theme change', async () => {
    (window as any).picto = { startupTheme: { requested: 'light' } };
    let complete!: (value: { colorScheme: string }) => void;
    mocks.getSettings.mockReturnValue(new Promise((resolve) => { complete = resolve; }));
    const stop = startThemeRuntime();
    previewTheme('purple', false);
    complete({ colorScheme: 'light' });
    await Promise.resolve();
    expect(document.documentElement.dataset.theme).toBe('purple');
    stop();
  });
  it('resolves auto and rejects platform-native themes on unsupported platforms', () => {
    expect(resolveTheme('auto', false, 'linux')).toMatchObject({ applied: 'light', colorScheme: 'light' });
    expect(resolveTheme('auto', true, 'linux')).toMatchObject({ applied: 'dark', colorScheme: 'dark' });
    expect(resolveTheme('vibrancy', false, 'windows').applied).toBe('dark');
    expect(resolveTheme('mica', false, 'mac').applied).toBe('dark');
    expect(resolveTheme('vibrancy', false, 'mac').applied).toBe('vibrancy');
  });

  it('applies every document theme surface from one function', () => {
    applyTheme('lightgray', true, 'mac');
    expect(document.documentElement.dataset.platform).toBe('mac');
    expect(document.documentElement.dataset.theme).toBe('lightgray');
    expect(document.documentElement.dataset.mantineColorScheme).toBe('light');
    expect(document.documentElement.style.colorScheme).toBe('light');
  });

  it.each(['mac', 'windows', 'linux'] as const)('publishes the %s font platform for CSS resolution', (platform) => {
    publishPlatform(platform);
    expect(document.documentElement.dataset.platform).toBe(platform);
  });

  it('installs one OS listener and one cross-window listener per active runtime', async () => {
    const first = startThemeRuntime();
    const second = startThemeRuntime();
    await Promise.resolve();
    expect(mocks.subscribeOsThemeChanged).toHaveBeenCalledTimes(2);
    expect(mocks.subscribeThemePreview).toHaveBeenCalledTimes(2);
    first();
    second();
  });

  it('restarts native material windows only when crossing a native-theme boundary', () => {
    expect(themeNeedsNativeWindowRestart('dark', 'blue')).toBe(false);
    expect(themeNeedsNativeWindowRestart('dark', 'vibrancy')).toBe(true);
    expect(themeNeedsNativeWindowRestart('vibrancy', 'dark')).toBe(true);
    expect(themeNeedsNativeWindowRestart('vibrancy', 'vibrancy')).toBe(false);
  });
});
