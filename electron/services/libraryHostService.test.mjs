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
    cloudLibraryId: 'cloud-library-id',
    icon: null,
  });

  expect(saved.libraryMeta['/Pictures/Test.library'].imageHash).toBe('image-hash');
  expect(saved.libraryMeta['/Pictures/Test.library'].imageFocusX).toBe(320);
  expect(saved.libraryMeta['/Pictures/Test.library'].imageFocusY).toBe(610);
  expect(saved.libraryMeta['/Pictures/Test.library'].imageZoomPercent).toBe(145);
  expect(saved.libraryMeta['/Pictures/Test.library'].cloudLibraryId).toBe('cloud-library-id');
  expect(saved.libraryMeta['/Pictures/Test.library'].icon).toBeNull();
  expect(events).toEqual([[
    'library-meta-changed',
    { path: '/Pictures/Test.library' },
  ]]);
});

test('open dialog accepts a macOS library package as well as a directory', async () => {
  const openDialogCalls = [];
  const opened = [];
  const service = createLibraryHostService({
    fs: {},
    path,
    dialog: {
      showOpenDialog: async (options) => {
        openDialogCalls.push(options);
        return { canceled: false, filePaths: ['/Pictures/Main.library'] };
      },
    },
    openLibrary: async (libraryPath) => opened.push(libraryPath),
    closeLibrary: async () => {},
    addLibraryToHistory: async () => {},
    removeLibraryFromHistory: async () => {},
    togglePinned: async () => {},
    getCachedConfig: () => ({}),
    saveGlobalConfig: async () => {},
    updateLibraryPath: async () => {},
    getCurrentLibraryRoot: () => null,
    setCurrentLibraryRoot: () => {},
    createMainWindow: () => {},
    sendToAllWindows: () => {},
    buildAppMenu: () => {},
  });

  await expect(service.openLibraryDialog()).resolves.toBe('/Pictures/Main.library');
  expect(openDialogCalls[0].properties).toEqual(['openFile', 'openDirectory']);
  expect(opened).toEqual(['/Pictures/Main.library']);
});

test('opening a library installs standard Windows folder icon metadata', async () => {
  const copies = [];
  const writes = [];
  const attributes = [];
  const winPath = path.win32;
  const libraryPath = 'C:\\Libraries\\Main.library';
  const service = createLibraryHostService({
    fs: {
      copyFile: async (...args) => copies.push(args),
      writeFile: async (...args) => writes.push(args),
    },
    path: winPath,
    dialog: {},
    openLibrary: async () => {},
    closeLibrary: async () => {},
    addLibraryToHistory: async () => {},
    removeLibraryFromHistory: async () => {},
    togglePinned: async () => {},
    getCachedConfig: () => ({}),
    saveGlobalConfig: async () => {},
    updateLibraryPath: async () => {},
    getCurrentLibraryRoot: () => null,
    setCurrentLibraryRoot: () => {},
    createMainWindow: () => {},
    sendToAllWindows: () => {},
    buildAppMenu: () => {},
    platform: 'win32',
    resourcesPath: 'C:\\Picto\\resources',
    isDefaultApp: false,
    runFileAttributeCommand: async (...args) => attributes.push(args),
  });

  await service.switchLibrary(libraryPath);

  const iconPath = `${libraryPath}\\.picto-library.ico`;
  const desktopIniPath = `${libraryPath}\\desktop.ini`;
  expect(copies).toEqual([['C:\\Picto\\resources\\library-icons\\library.ico', iconPath]]);
  expect(writes).toEqual([[
    desktopIniPath,
    '[.ShellClassInfo]\r\nIconResource=.picto-library.ico,0\r\n',
    'utf8',
  ]]);
  expect(attributes).toEqual([
    ['attrib', ['+h', '+s', iconPath]],
    ['attrib', ['+h', '+s', desktopIniPath]],
    ['attrib', ['+r', libraryPath]],
  ]);
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
