import { expect, test } from 'vitest';
import { createLibraryHostService } from './libraryHostService.mjs';
import path from 'node:path';

test('library image metadata is persisted and broadcast to every window', async () => {
  const config = { libraryMeta: {} };
  let saved = null;
  const events = [];
  const service = createLibraryHostService({
    fs: {},
    path: {},
    dialog: {},
    openLibrary: async () => {},
    closeLibrary: async () => {},
    addLibraryToHistory: async () => {},
    removeLibraryFromHistory: async () => {},
    togglePinned: async () => {},
    getCachedConfig: () => config,
    saveGlobalConfig: async (next) => { saved = next; },
    updateLibraryPath: async () => {},
    getCurrentLibraryRoot: () => '/Pictures/Test.library',
    setCurrentLibraryRoot: () => {},
    createMainWindow: () => {},
    sendToAllWindows: (name, payload) => events.push([name, payload]),
    buildAppMenu: () => {},
  });

  await service.setLibraryMeta('/Pictures/Test.library', {
    imageHash: 'image-hash',
    imageFocusX: 320,
    imageFocusY: 610,
    imageZoomPercent: 145,
    icon: null,
  });

  expect(saved.libraryMeta['/Pictures/Test.library'].imageHash).toBe('image-hash');
  expect(saved.libraryMeta['/Pictures/Test.library'].imageFocusX).toBe(320);
  expect(saved.libraryMeta['/Pictures/Test.library'].imageFocusY).toBe(610);
  expect(saved.libraryMeta['/Pictures/Test.library'].imageZoomPercent).toBe(145);
  expect(saved.libraryMeta['/Pictures/Test.library'].icon).toBeNull();
  expect(events).toEqual([[
    'library-meta-changed',
    { path: '/Pictures/Test.library' },
  ]]);
});

test('guided tour opens an unpersisted isolated library and restores the original', async () => {
  const events = [];
  const calls = [];
  let current = '/Pictures/Main.library';
  const histories = [];
  let tutorialNumber = 0;
  const service = createLibraryHostService({
    fs: {
      readdir: async () => [],
      mkdtemp: async () => `/tmp/picto-guided-tour-test-${++tutorialNumber}`,
      mkdir: async () => {},
      rm: async (...args) => calls.push(['rm', ...args]),
    },
    path,
    dialog: {},
    openLibrary: async (libraryPath) => calls.push(['open', libraryPath]),
    openTutorialLibrary: async (...args) => calls.push(['open-tutorial', ...args]),
    closeLibrary: async () => calls.push(['close']),
    invokeSerialized: async (command) => JSON.stringify(
      command === 'folders.create' ? { folder_id: calls.filter(([name]) => name === 'invoke').length + 1 }
        : command === 'subscriptions.create' ? { subscription_id: 7 }
          : {},
    ),
    addLibraryToHistory: async (value) => histories.push(value),
    removeLibraryFromHistory: async () => {},
    togglePinned: async () => {},
    getCachedConfig: () => ({ libraryHistory: ['/Pictures/Main.library'] }),
    saveGlobalConfig: async () => {},
    updateLibraryPath: async () => {},
    getCurrentLibraryRoot: () => current,
    setCurrentLibraryRoot: (value) => { current = value; },
    createMainWindow: () => {},
    sendToAllWindows: (name, payload) => events.push([name, payload]),
    buildAppMenu: () => {},
    tutorialRoot: '/tmp',
    tutorialFixtureRoot: '/app/tutorial',
  });

  const started = await service.startTutorialLibrary();
  expect(started.path).toBe('/tmp/picto-guided-tour-test-1/Guided Tour.library');
  expect(current).toBe(started.path);
  expect(histories).toEqual([]);
  expect(calls).toContainEqual(['open-tutorial', started.path, '/app/tutorial']);

  const reset = await service.resetTutorialLibrary();
  expect(reset.path).toBe('/tmp/picto-guided-tour-test-2/Guided Tour.library');
  expect(current).toBe(reset.path);
  expect(histories).toEqual([]);
  expect(calls).toContainEqual(['rm', '/tmp/picto-guided-tour-test-1', { recursive: true, force: true }]);

  await service.finishTutorialLibrary();
  expect(current).toBe('/Pictures/Main.library');
  expect(histories).toEqual([]);
  expect(calls).toContainEqual(['open', '/Pictures/Main.library']);
  expect(calls).toContainEqual(['rm', '/tmp/picto-guided-tour-test-2', { recursive: true, force: true }]);
  expect(events.at(-1)).toEqual(['library-switched', { path: '/Pictures/Main.library', restored: true }]);
});
