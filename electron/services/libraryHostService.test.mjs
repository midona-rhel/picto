import { expect, test } from 'vitest';
import { createLibraryHostService } from './libraryHostService.mjs';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

test('remembers a manually selected cloud root once', async () => {
  const config = { cloudLocations: {} };
  let saved = null;
  const service = createLibraryHostService({
    fs: {}, path, dialog: {}, openLibrary: async () => {}, closeLibrary: async () => {},
    addLibraryToHistory: async () => {}, removeLibraryFromHistory: async () => {}, togglePinned: async () => {},
    getCachedConfig: () => config,
    saveGlobalConfig: async (next) => { saved = next; },
    updateLibraryPath: async () => {}, getCurrentLibraryRoot: () => null, setCurrentLibraryRoot: () => {},
    createMainWindow: () => {}, sendToAllWindows: () => {}, buildAppMenu: () => {},
  });
  const root = { provider: 'google_drive', account_label: 'My Drive', path: 'G:\\My Drive' };

  await service.rememberCloudRoot(root);
  await service.rememberCloudRoot(root);

  expect(saved.cloudLocations).toEqual({ google_drive: root });
});

test('keeps only one selected root for each cloud provider', async () => {
  const dropbox = { provider: 'dropbox', account_label: 'Dropbox', path: 'D:\\Dropbox' };
  const oldDrive = { provider: 'google_drive', account_label: 'Old', path: 'G:\\My Drive' };
  const newDrive = { provider: 'google_drive', account_label: 'New', path: 'H:\\My Drive' };
  const config = { cloudLocations: { google_drive: oldDrive, dropbox } };
  const service = createLibraryHostService({
    fs: {}, path, dialog: {}, openLibrary: async () => {}, closeLibrary: async () => {},
    addLibraryToHistory: async () => {}, removeLibraryFromHistory: async () => {}, togglePinned: async () => {},
    getCachedConfig: () => config,
    saveGlobalConfig: async () => {},
    updateLibraryPath: async () => {}, getCurrentLibraryRoot: () => null, setCurrentLibraryRoot: () => {},
    createMainWindow: () => {}, sendToAllWindows: () => {}, buildAppMenu: () => {},
  });

  await expect(service.rememberCloudRoot(newDrive)).resolves.toEqual({ google_drive: newDrive, dropbox });
});

test('an initial navigation failure closes the library and preserves its error for Library Manager', async () => {
  const events = [];
  let currentPath = '/Pictures/Broken.library';
  let closeCalls = 0;
  const service = createLibraryHostService({
    fs: { access: async () => {} },
    path,
    dialog: {},
    openLibrary: async () => {},
    closeLibrary: async () => { closeCalls += 1; },
    addLibraryToHistory: async () => {},
    removeLibraryFromHistory: async () => {},
    togglePinned: async () => {},
    getCachedConfig: () => ({ libraryHistory: ['/Pictures/Broken.library'] }),
    saveGlobalConfig: async () => {},
    updateLibraryPath: async () => {},
    getCurrentLibraryRoot: () => currentPath,
    setCurrentLibraryRoot: (value) => { currentPath = value; },
    createMainWindow: () => {},
    sendToAllWindows: (name, payload) => events.push([name, payload]),
    buildAppMenu: () => {},
  });

  await service.failActiveLibrary('database is unreadable');

  expect(closeCalls).toBe(1);
  expect(currentPath).toBeNull();
  expect(events).toContainEqual([
    'library-open-failed',
    { path: '/Pictures/Broken.library', message: 'database is unreadable' },
  ]);
  await expect(service.getLibraryConfig()).resolves.toMatchObject({
    currentPath: null,
    libraryFailure: { path: '/Pictures/Broken.library', message: 'database is unreadable' },
  });
});

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

test('library covers are materialized in the library root', async () => {
  const libraryPath = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-library-cover-'));
  const imageHash = 'a'.repeat(64);
  const thumbnail = path.join(libraryPath, 'blobs', 't', 'aa', 'aa', `${imageHash}.jpg`);
  try {
    await fs.mkdir(path.dirname(thumbnail), { recursive: true });
    await fs.writeFile(thumbnail, 'cover bytes');
    const config = { libraryMeta: {} };
    const service = createLibraryHostService({
      fs,
      path,
      dialog: {},
      openLibrary: async () => {},
      closeLibrary: async () => {},
      addLibraryToHistory: async () => {},
      removeLibraryFromHistory: async () => {},
      togglePinned: async () => {},
      getCachedConfig: () => config,
      saveGlobalConfig: async () => {},
      updateLibraryPath: async () => {},
      getCurrentLibraryRoot: () => libraryPath,
      setCurrentLibraryRoot: () => {},
      createMainWindow: () => {},
      sendToAllWindows: () => {},
      buildAppMenu: () => {},
    });

    await service.setLibraryMeta(libraryPath, { imageHash });

    expect(await fs.readFile(path.join(libraryPath, '.picto-library-cover.jpg'), 'utf8')).toBe('cover bytes');
    expect(config.libraryMeta[libraryPath].imageHash).toBe(imageHash);
  } finally {
    await fs.rm(libraryPath, { recursive: true, force: true });
  }
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

test('opening a library applies the persistent macOS package icon', async () => {
  const icons = [];
  const libraryPath = '/Pictures/Main.library';
  const service = createLibraryHostService({
    fs: {},
    path,
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
    platform: 'darwin',
    resourcesPath: '/Applications/Picto.app/Contents/Resources',
    isDefaultApp: false,
    setFileIcon: async (...args) => {
      icons.push(args);
      return true;
    },
  });

  await service.switchLibrary(libraryPath);

  expect(icons).toEqual([[
    '/Applications/Picto.app/Contents/Resources/library-icons/library.icns',
    libraryPath,
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

test('forced startup libraries do not replace the user last-opened library', async () => {
  const histories = [];
  const opened = [];
  let current = null;
  const service = createLibraryHostService({
    fs: { readdir: async () => [] },
    path,
    dialog: {},
    openLibrary: async (libraryPath) => opened.push(libraryPath),
    closeLibrary: async () => {},
    addLibraryToHistory: async (libraryPath) => histories.push(libraryPath),
    removeLibraryFromHistory: async () => {},
    togglePinned: async () => {},
    getCachedConfig: () => ({}),
    saveGlobalConfig: async () => {},
    updateLibraryPath: async () => {},
    getCurrentLibraryRoot: () => current,
    setCurrentLibraryRoot: (value) => { current = value; },
    createMainWindow: () => {},
    sendToAllWindows: () => {},
    buildAppMenu: () => {},
  });

  await service.initializeInitialLibrary('/tmp/automation.library', { remember: false });

  expect(opened).toEqual(['/tmp/automation.library']);
  expect(current).toBe('/tmp/automation.library');
  expect(histories).toEqual([]);
});
