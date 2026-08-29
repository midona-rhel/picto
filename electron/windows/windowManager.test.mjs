import { expect, test } from 'vitest';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { windowResizePersistenceEvent } from './windowManager.mjs';

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
