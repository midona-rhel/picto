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
  resolveTheme,
  startThemeRuntime,
  themeNeedsNativeWindowRestart,
} from './themeRuntime';

beforeEach(() => {
  vi.clearAllMocks();
  mocks.getSettings.mockResolvedValue({ colorScheme: 'dark' });
  mocks.subscribeThemePreview.mockResolvedValue(() => {});
  mocks.subscribeOsThemeChanged.mockResolvedValue(() => {});
  mocks.publishThemePreview.mockResolvedValue(undefined);
});

describe('themeRuntime', () => {
  it('resolves auto and rejects platform-native themes on unsupported platforms', () => {
    expect(resolveTheme('auto', false, 'linux')).toMatchObject({ applied: 'light', colorScheme: 'light' });
    expect(resolveTheme('auto', true, 'linux')).toMatchObject({ applied: 'dark', colorScheme: 'dark' });
    expect(resolveTheme('vibrancy', false, 'windows').applied).toBe('dark');
    expect(resolveTheme('mica', false, 'mac').applied).toBe('dark');
    expect(resolveTheme('vibrancy', false, 'mac').applied).toBe('vibrancy');
  });

  it('applies every document theme surface from one function', () => {
    applyTheme('lightgray', true);
    expect(document.documentElement.dataset.theme).toBe('lightgray');
    expect(document.documentElement.dataset.mantineColorScheme).toBe('light');
    expect(document.documentElement.style.colorScheme).toBe('light');
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
