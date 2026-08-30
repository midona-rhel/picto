import { expect, test } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { resolveThemeInfo, windowResizePersistenceEvent } from './windowManager.mjs';

test('resolves the globally persisted theme before creating a window', () => {
  expect(resolveThemeInfo('purple')).toEqual({ theme: 'purple', bgColor: '#1e1526' });
  expect(resolveThemeInfo('auto', false)).toEqual({ theme: 'light', bgColor: '#ebedef' });
  expect(resolveThemeInfo('auto', true)).toEqual({ theme: 'dark', bgColor: '#1a1a1e' });
});

test('persists bounds only after native resize settles on macOS and Windows', () => {
  expect(windowResizePersistenceEvent('darwin')).toBe('resized');
  expect(windowResizePersistenceEvent('win32')).toBe('resized');
});

test('uses the debounced continuous resize fallback on Linux', () => {
  expect(windowResizePersistenceEvent('linux')).toBe('resize');
});

test('keeps settings on the native macOS resize path', () => {
  const source = readFileSync(resolve(process.cwd(), 'electron/windows/windowManager.mjs'), 'utf8');
  expect(source).toContain("titleBarStyle: 'hiddenInset'");
  expect(source).toContain('if (isSettings && isMac)');
  expect(source).toContain('win.setWindowButtonVisibility(false)');
  expect(source).toMatch(/minHeight: 650,[\s\S]*?resizable: true/);
});

test('clips the transparent Library Manager surface to rounded corners', () => {
  const source = readFileSync(resolve(process.cwd(), 'src/features/library/LibraryManager.module.css'), 'utf8');
  expect(source).toContain('border-radius: 10px');
  expect(source).toContain('clip-path: inset(0 round 10px)');
});
