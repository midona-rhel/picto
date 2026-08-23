import { expect, test } from 'vitest';
import { windowResizePersistenceEvent } from './windowManager.mjs';

test('persists bounds only after native resize settles on macOS and Windows', () => {
  expect(windowResizePersistenceEvent('darwin')).toBe('resized');
  expect(windowResizePersistenceEvent('win32')).toBe('resized');
});

test('uses the debounced continuous resize fallback on Linux', () => {
  expect(windowResizePersistenceEvent('linux')).toBe('resize');
});
