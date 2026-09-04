import { expect, test, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import {
  calcDetailWindowAspectRatio,
  resolveThemeInfo,
  windowResizePersistenceEvent,
  createWindowManager,
} from './windowManager.mjs';

test('resolves native window backgrounds to the library theme colors', () => {
  expect(resolveThemeInfo('purple')).toEqual({ theme: 'purple', bgColor: '#1c1424' });
  expect(resolveThemeInfo('auto', false)).toEqual({ theme: 'light', bgColor: '#ffffff' });
  expect(resolveThemeInfo('auto', true)).toEqual({ theme: 'dark', bgColor: '#18191c' });
});

test.each(['darwin', 'win32', 'linux'])('uses supported native themes on %s', (platform) => {
  expect(resolveThemeInfo('mica', false, platform).theme).toBe(platform === 'win32' ? 'mica' : 'dark');
  expect(resolveThemeInfo('vibrancy', false, platform).theme).toBe(platform === 'darwin' ? 'vibrancy' : 'dark');
});

test('preload and reload snapshots follow the current library, never stale global config', async () => {
  let theme = 'light';
  const save = vi.fn();
  const nativeTheme = { shouldUseDarkColors: false };
  const manager = createWindowManager({
    BrowserWindow: { getAllWindows: () => [] },
    invoke: async () => ({ value: { colorScheme: theme } }),
    getCachedConfig: () => ({ theme: 'purple', lastLibrary: '/old/library' }),
    saveGlobalConfig: save,
    nativeTheme,
  });
  await manager.loadLibraryTheme();
  expect(manager.getStartupTheme()).toMatchObject({ requested: 'light', applied: 'light', colorScheme: 'light', backgroundColor: '#ffffff' });
  theme = 'blue';
  await manager.loadLibraryTheme();
  expect(manager.getStartupTheme()).toMatchObject({ requested: 'blue', applied: 'blue', colorScheme: 'dark' });
  theme = 'auto';
  await manager.loadLibraryTheme();
  nativeTheme.shouldUseDarkColors = true;
  expect(manager.getStartupTheme()).toMatchObject({ requested: 'auto', applied: 'dark' });
  expect(save).not.toHaveBeenCalled();
});

test('persists bounds only after native resize settles on macOS and Windows', () => {
  expect(windowResizePersistenceEvent('darwin')).toBe('resized');
  expect(windowResizePersistenceEvent('win32')).toBe('resized');
});

test('uses the debounced continuous resize fallback on Linux', () => {
  expect(windowResizePersistenceEvent('linux')).toBe('resize');
});

test('uses the opened media aspect ratio for detail windows', () => {
  expect(calcDetailWindowAspectRatio(3840, 2160)).toBeCloseTo(16 / 9);
  expect(calcDetailWindowAspectRatio('1200', '1600')).toBe(0.75);
  expect(calcDetailWindowAspectRatio(0, 1600)).toBeNull();
  expect(calcDetailWindowAspectRatio(undefined, undefined)).toBeNull();

  const source = readFileSync(resolve(process.cwd(), 'electron/windows/windowManager.mjs'), 'utf8');
  expect(source).toContain('win.setAspectRatio(detailAspectRatio)');
});

test('keeps settings on the native macOS resize path', () => {
  const source = readFileSync(resolve(process.cwd(), 'electron/windows/windowManager.mjs'), 'utf8');
  expect(source).toContain("titleBarStyle: 'hiddenInset'");
  expect(source).toContain('if ((isSettings || isLibraryManager) && isMac)');
  expect(source).toContain('win.setWindowButtonVisibility(false)');
  expect(source).toMatch(/minHeight: 650,[\s\S]*?resizable: true/);
});

test('uses the same solid standalone window treatment for Library Manager as Settings', () => {
  const manager = readFileSync(resolve(process.cwd(), 'electron/windows/windowManager.mjs'), 'utf8');
  const source = readFileSync(resolve(process.cwd(), 'src/features/library/LibraryManager.module.css'), 'utf8');
  expect(manager).toMatch(/isLibraryManager[\s\S]*?transparent: false/);
  expect(manager).not.toMatch(/parent: mainWin, modal: true/);
  expect(source).toContain('--manager-shell-bg');
  expect(source).not.toContain('backdrop-filter: var(--glass-blur)');
});
