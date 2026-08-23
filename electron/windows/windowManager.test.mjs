import test from 'node:test';
import assert from 'node:assert/strict';
import { windowResizePersistenceEvent } from './windowManager.mjs';

test('persists bounds only after native resize settles on macOS and Windows', () => {
  assert.equal(windowResizePersistenceEvent('darwin'), 'resized');
  assert.equal(windowResizePersistenceEvent('win32'), 'resized');
});

test('uses the debounced continuous resize fallback on Linux', () => {
  assert.equal(windowResizePersistenceEvent('linux'), 'resize');
});
