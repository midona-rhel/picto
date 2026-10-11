import { expect, test, vi } from 'vitest';
import { createLibraryHostService } from './libraryHostService.mjs';
import { LIBRARY_OPEN_CANCELLED } from '../libraryOpenCancellation.mjs';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

test('refuses to create a library inside another library package', async () => {
  const service = createLibraryHostService({
    fs: { mkdir: async () => { throw new Error('must not create directories'); } },
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
  });

  await expect(service.createLibrary({
    name: 'Nested',
    savePath: '/Pictures/Main.library/Exports',
  })).rejects.toThrow('cannot be created inside another Picto library');
});

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
    expect(JSON.parse(await fs.readFile(path.join(libraryPath, '.picto-library-cover.json'), 'utf8'))).toEqual({
      imageHash,
      imageFocusX: 500,
      imageFocusY: 500,
      imageZoomPercent: 100,
    });
    expect(config.libraryMeta[libraryPath].imageHash).toBe(imageHash);
  } finally {
    await fs.rm(libraryPath, { recursive: true, force: true });
  }
});

test('an existing root cover remains visible when app metadata was lost', async () => {
  const libraryPath = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-library-root-cover-'));
  try {
    await fs.writeFile(path.join(libraryPath, 'library.sqlite'), 'database');
    await fs.writeFile(path.join(libraryPath, '.picto-library-cover.jpg'), 'cover bytes');
    const service = createLibraryHostService({
      fs, path, dialog: {}, openLibrary: async () => {}, closeLibrary: async () => {},
      addLibraryToHistory: async () => {}, removeLibraryFromHistory: async () => {}, togglePinned: async () => {},
      getCachedConfig: () => ({ libraryHistory: [libraryPath] }), saveGlobalConfig: async () => {},
      updateLibraryPath: async () => {}, getCurrentLibraryRoot: () => libraryPath, setCurrentLibraryRoot: () => {},
      createMainWindow: () => {}, sendToAllWindows: () => {}, buildAppMenu: () => {},
    });

    await expect(service.getLibraryConfig()).resolves.toMatchObject({
      coverExistsMap: { [libraryPath]: true },
    });
  } finally {
    await fs.rm(libraryPath, { recursive: true, force: true });
  }
});

test('root cover metadata restores the selected crop after app metadata was lost', async () => {
  const libraryPath = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-library-root-crop-'));
  const imageHash = 'c'.repeat(64);
  const config = { libraryHistory: [libraryPath] };
  try {
    await fs.writeFile(path.join(libraryPath, 'library.sqlite'), 'database');
    await fs.writeFile(path.join(libraryPath, '.picto-library-cover.jpg'), 'cover bytes');
    await fs.writeFile(path.join(libraryPath, '.picto-library-cover.json'), JSON.stringify({
      imageHash,
      imageFocusX: 275,
      imageFocusY: 725,
      imageZoomPercent: 160,
    }));
    const service = createLibraryHostService({
      fs, path, dialog: {}, openLibrary: async () => {}, closeLibrary: async () => {},
      addLibraryToHistory: async () => {}, removeLibraryFromHistory: async () => {}, togglePinned: async () => {},
      getCachedConfig: () => config, saveGlobalConfig: async () => {},
      updateLibraryPath: async () => {}, getCurrentLibraryRoot: () => libraryPath, setCurrentLibraryRoot: () => {},
      createMainWindow: () => {}, sendToAllWindows: () => {}, buildAppMenu: () => {},
    });

    await expect(service.getLibraryConfig()).resolves.toMatchObject({
      libraryMeta: {
        [libraryPath]: {
          imageHash,
          imageFocusX: 275,
          imageFocusY: 725,
          imageZoomPercent: 160,
        },
      },
    });
  } finally {
    await fs.rm(libraryPath, { recursive: true, force: true });
  }
});

test('cover materialization merges into config saved while the file copy was running', async () => {
  const libraryPath = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-library-cover-race-'));
  const imageHash = 'b'.repeat(64);
  const thumbnail = path.join(libraryPath, 'blobs', 't', 'bb', 'bb', `${imageHash}.jpg`);
  let config = { libraryMeta: {} };
  let releaseCopy;
  const copyReleased = new Promise((resolve) => { releaseCopy = resolve; });
  let markCopyStarted;
  const copyStarted = new Promise((resolve) => { markCopyStarted = resolve; });
  const delayedFs = new Proxy(fs, {
    get(target, property) {
      if (property !== 'copyFile') return target[property];
      return async (...args) => {
        markCopyStarted();
        await copyReleased;
        return target.copyFile(...args);
      };
    },
  });
  try {
    await fs.mkdir(path.dirname(thumbnail), { recursive: true });
    await fs.writeFile(thumbnail, 'cover bytes');
    const service = createLibraryHostService({
      fs: delayedFs, path, dialog: {}, openLibrary: async () => {}, closeLibrary: async () => {},
      addLibraryToHistory: async () => {}, removeLibraryFromHistory: async () => {}, togglePinned: async () => {},
      getCachedConfig: () => config, saveGlobalConfig: async (next) => { config = next; },
      updateLibraryPath: async () => {}, getCurrentLibraryRoot: () => libraryPath, setCurrentLibraryRoot: () => {},
      createMainWindow: () => {}, sendToAllWindows: () => {}, buildAppMenu: () => {},
    });

    const savingCover = service.setLibraryMeta(libraryPath, { imageHash });
    await copyStarted;
    config = { ...config, theme: 'dark' };
    releaseCopy();
    await savingCover;

    expect(config.theme).toBe('dark');
    expect(config.libraryMeta[libraryPath].imageHash).toBe(imageHash);
  } finally {
    await fs.rm(libraryPath, { recursive: true, force: true });
  }
});

test('open dialog accepts a macOS library package as well as a directory', async () => {
  const openDialogCalls = [];
  const opened = [];
  const service = createLibraryHostService({
    platform: 'darwin',
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

test('delete revalidates library ownership after confirmation before removing anything', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-delete-ownership-'));
  const libraryPath = path.join(root, 'Remembered.library');
  const removedFromHistory = vi.fn();
  try {
    await fs.mkdir(libraryPath);
    await fs.writeFile(path.join(libraryPath, 'library.sqlite'), 'database');
    const { service } = createOpenService(createNativeOpenStub(), {
      fs,
      path,
      dialog: {
        showMessageBox: async () => {
          await fs.rm(path.join(libraryPath, 'library.sqlite'));
          await fs.writeFile(path.join(libraryPath, 'important.txt'), 'unrelated data');
          return { response: 0 };
        },
      },
      removeLibraryFromHistory: removedFromHistory,
    });

    await expect(service.deleteLibrary(libraryPath)).rejects.toThrow('no longer a Picto library');
    await expect(fs.readFile(path.join(libraryPath, 'important.txt'), 'utf8')).resolves.toBe('unrelated data');
    expect(removedFromHistory).not.toHaveBeenCalled();
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
});

test('delete refuses a symlink even when its target is a Picto library', async () => {
  const removed = vi.fn();
  const { service } = createOpenService(createNativeOpenStub(), {
    fs: {
      lstat: async () => ({ isDirectory: () => false, isSymbolicLink: () => true }),
      realpath: async () => '/Pictures/Actual.library',
      rm: removed,
    },
    dialog: { showMessageBox: async () => ({ response: 0 }) },
  });

  await expect(service.deleteLibrary('/Pictures/Remembered.library')).rejects.toThrow('no longer a Picto library');
  expect(removed).not.toHaveBeenCalled();
});

test('delete refuses a symlinked library database ownership marker', async () => {
  const removed = vi.fn();
  const libraryPath = '/Pictures/Remembered.library';
  const { service } = createOpenService(createNativeOpenStub(), {
    fs: {
      lstat: async (target) => (target === libraryPath
        ? { isDirectory: () => true, isSymbolicLink: () => false }
        : { isFile: () => false, isSymbolicLink: () => true }),
      realpath: async () => libraryPath,
      rm: removed,
    },
    dialog: { showMessageBox: async () => ({ response: 0 }) },
  });

  await expect(service.deleteLibrary(libraryPath)).rejects.toThrow('no longer a Picto library');
  expect(removed).not.toHaveBeenCalled();
});

test('delete rechecks the current library by real path after the confirmation dialog', async () => {
  const libraryPath = '/Pictures/Owned.library';
  const aliasPath = '/Volumes/Pictures Alias/Owned.library';
  const removed = vi.fn();
  let currentPath = null;
  const { service } = createOpenService(createNativeOpenStub(), {
    fs: {
      lstat: async (target) => (target === libraryPath
        ? { isDirectory: () => true, isSymbolicLink: () => false }
        : { isFile: () => true }),
      realpath: async (target) => (target === aliasPath ? libraryPath : target),
      rm: removed,
    },
    dialog: {
      showMessageBox: async () => {
        currentPath = aliasPath;
        return { response: 0 };
      },
    },
    getCurrentLibraryRoot: () => currentPath,
  });

  await expect(service.deleteLibrary(libraryPath)).rejects.toThrow('currently open');
  expect(removed).not.toHaveBeenCalled();
});

test('delete removes a revalidated Picto library and then forgets it', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-delete-library-'));
  const libraryPath = path.join(root, 'Owned.library');
  const removedFromHistory = vi.fn();
  try {
    await fs.mkdir(libraryPath);
    await fs.writeFile(path.join(libraryPath, 'library.sqlite'), 'database');
    await fs.writeFile(path.join(libraryPath, 'metadata.json'), '{}');
    const { service } = createOpenService(createNativeOpenStub(), {
      fs,
      path,
      dialog: { showMessageBox: async () => ({ response: 0 }) },
      removeLibraryFromHistory: removedFromHistory,
    });

    await expect(service.deleteLibrary(libraryPath)).resolves.toEqual({ deleted: true });
    await expect(fs.access(libraryPath)).rejects.toMatchObject({ code: 'ENOENT' });
    expect(removedFromHistory).toHaveBeenCalledWith(libraryPath);
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
});

test('delete refuses a library whose native open is still pending after confirmation', async () => {
  const libraryPath = '/Pictures/Owned.library';
  const aliasPath = '/Volumes/Alias/Owned.library';
  const native = createNativeOpenStub();
  const removed = vi.fn();
  let switching;
  const { service, state } = createOpenService(native, {
    fs: {
      lstat: async (target) => (target === libraryPath
        ? { isDirectory: () => true, isSymbolicLink: () => false }
        : { isFile: () => true }),
      realpath: async (target) => target === aliasPath ? libraryPath : target,
      rm: removed,
    },
    dialog: { showMessageBox: async () => {
      switching = service.switchLibrary(aliasPath);
      await flush();
      return { response: 0 };
    } },
  });
  try {
    await expect(service.deleteLibrary(libraryPath)).rejects.toThrow('currently open');
    expect(state.current).toBeNull();
    expect(native.opening()).toEqual([aliasPath]);
    expect(removed).not.toHaveBeenCalled();
  } finally {
    native.complete(aliasPath);
    await switching;
  }
});

test.each([false, true])('deletion reserves its package until removal settles (failure=%s)', async (failRemoval) => {
  const libraryPath = '/Pictures/Owned.library';
  const aliasPath = '/Volumes/Alias/Owned.library';
  const native = createNativeOpenStub();
  let finishRemoval;
  const removal = new Promise((resolve, reject) => {
    finishRemoval = () => failRemoval ? reject(new Error('removal failed')) : resolve();
  });
  const rm = vi.fn(() => removal);
  const { service } = createOpenService(native, {
    fs: {
      lstat: async (target) => (target === libraryPath
        ? { isDirectory: () => true, isSymbolicLink: () => false }
        : { isFile: () => true }),
      realpath: async (target) => target === aliasPath ? libraryPath : target,
      rm,
    },
    dialog: { showMessageBox: async () => ({ response: 0 }) },
  });
  const deleting = service.deleteLibrary(libraryPath);
  const outcome = deleting.catch((error) => error.message);
  await flush();
  expect(rm).toHaveBeenCalledOnce();
  const switching = service.switchLibrary(aliasPath).catch((error) => error.message);
  await flush();
  // Settle a wrongly allowed native open too, so the pre-fix regression cannot hang.
  if (native.opening().length) native.complete(aliasPath);
  expect(await switching).toMatch(/deletion/);
  expect(native.calls).toEqual([]);
  finishRemoval();
  expect(await outcome).toEqual(failRemoval ? 'removal failed' : { deleted: true });
  // No stale reservation may block a subsequent open after success or failure.
  const retry = service.switchLibrary(aliasPath);
  await flush();
  native.complete(aliasPath);
  await retry;
});

test('relocation rejects a symlink destination inside another library', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-move-nested-alias-'));
  try {
    const source = path.join(root, 'A.library');
    const nested = path.join(root, 'B.library', 'exports');
    const alias = path.join(root, 'export');
    await fs.mkdir(source);
    await fs.writeFile(path.join(source, 'library.sqlite'), 'source');
    await fs.mkdir(nested, { recursive: true });
    await fs.symlink(nested, alias, process.platform === 'win32' ? 'junction' : 'dir');
    const { service } = createOpenService(createNativeOpenStub(), {
      fs, path,
      dialog: { showOpenDialog: async () => ({ canceled: false, filePaths: [alias] }) },
    });
    await expect(service.relocateLibrary(source)).rejects.toThrow('inside another Picto library');
    await expect(fs.readFile(path.join(source, 'library.sqlite'), 'utf8')).resolves.toBe('source');
    await expect(fs.readdir(nested)).resolves.toEqual([]);
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
});

test.each([
  [path.posix, '/Pictures/A.library', '/Pictures/B.library/exports'],
  [path.win32, 'C:\\Pictures\\A.library', 'D:\\B.library\\exports'],
])('relocation refuses a destination inside a library package (case %#)', async (pathApi, source, destination) => {
  const rename = vi.fn();
  const { service } = createOpenService(createNativeOpenStub(), {
    path: pathApi,
    fs: { access: async (target) => {
      if (target !== source) throw Object.assign(new Error('missing'), { code: 'ENOENT' });
    }, rename },
    dialog: { showOpenDialog: async () => ({ canceled: false, filePaths: [destination] }) },
  });
  await expect(service.relocateLibrary(source)).rejects.toThrow('inside another Picto library');
  expect(rename).not.toHaveBeenCalled();
});

test('a cross-volume relocation publishes a verified real copy before removing the source', async () => {
  const root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'picto-cross-volume-move-')));
  const source = path.join(root, 'source', 'A.library');
  const destinationDirectory = path.join(root, 'destination');
  const destination = path.join(destinationDirectory, 'A.library');
  const updates = vi.fn();
  try {
    await fs.mkdir(path.join(source, 'blobs', 'aa'), { recursive: true });
    await fs.mkdir(destinationDirectory);
    await fs.writeFile(path.join(source, 'library.sqlite'), 'database bytes');
    await fs.writeFile(path.join(source, 'blobs', 'aa', 'asset.bin'), 'asset bytes');
    const crossVolumeFs = new Proxy(fs, {
      get(target, property) {
        // Verification must not materialize file lists for the whole library.
        if (property === 'readdir') return () => { throw new Error('use bounded directory iteration'); };
        if (property !== 'rename') return target[property];
        return async (from, to) => {
          if (from === source && to === destination) {
            throw Object.assign(new Error('cross-device link'), { code: 'EXDEV' });
          }
          return target.rename(from, to);
        };
      },
    });
    const { service } = createOpenService(createNativeOpenStub(), {
      fs: crossVolumeFs,
      path,
      dialog: { showOpenDialog: async () => ({ canceled: false, filePaths: [destinationDirectory] }) },
      updateLibraryPath: updates,
    });

    await expect(service.relocateLibrary(source)).resolves.toEqual({ action: 'relocated', newPath: destination });
    await expect(fs.access(source)).rejects.toMatchObject({ code: 'ENOENT' });
    await expect(fs.readFile(path.join(destination, 'library.sqlite'), 'utf8')).resolves.toBe('database bytes');
    await expect(fs.readFile(path.join(destination, 'blobs', 'aa', 'asset.bin'), 'utf8')).resolves.toBe('asset bytes');
    expect(updates).toHaveBeenCalledWith(source, destination);
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
});

test('a failed cross-volume copy leaves the source intact and removes staging', async () => {
  const root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'picto-cross-volume-copy-failure-')));
  const source = path.join(root, 'source', 'A.library');
  const destinationDirectory = path.join(root, 'destination');
  const destination = path.join(destinationDirectory, 'A.library');
  const updates = vi.fn();
  try {
    await fs.mkdir(source, { recursive: true });
    await fs.mkdir(destinationDirectory);
    await fs.writeFile(path.join(source, 'library.sqlite'), 'database bytes');
    const failingFs = new Proxy(fs, {
      get(target, property) {
        if (property === 'rename') {
          return async (from, to) => {
            if (from === source && to === destination) {
              throw Object.assign(new Error('cross-device link'), { code: 'EXDEV' });
            }
            return target.rename(from, to);
          };
        }
        if (property === 'cp') {
          return async (_from, to) => {
            await target.mkdir(to, { recursive: true });
            await target.writeFile(path.join(to, 'partial'), 'partial');
            throw Object.assign(new Error('copy failed'), { code: 'EIO' });
          };
        }
        return target[property];
      },
    });
    const { service } = createOpenService(createNativeOpenStub(), {
      fs: failingFs,
      path,
      dialog: { showOpenDialog: async () => ({ canceled: false, filePaths: [destinationDirectory] }) },
      updateLibraryPath: updates,
    });

    await expect(service.relocateLibrary(source)).rejects.toThrow('copy failed');
    await expect(fs.readFile(path.join(source, 'library.sqlite'), 'utf8')).resolves.toBe('database bytes');
    await expect(fs.access(destination)).rejects.toMatchObject({ code: 'ENOENT' });
    await expect(fs.readdir(destinationDirectory)).resolves.toEqual([]);
    expect(updates).not.toHaveBeenCalled();
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
});

test.each(['changed bytes', 'extra entry', 'wrong entry type'])(
  'cross-volume verification rejects %s, preserves the source and removes staging', async (fault) => {
  const root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'picto-cross-volume-verify-failure-')));
  const source = path.join(root, 'source', 'A.library');
  const destinationDirectory = path.join(root, 'destination');
  const destination = path.join(destinationDirectory, 'A.library');
  try {
    await fs.mkdir(source, { recursive: true });
    await fs.mkdir(destinationDirectory);
    await fs.writeFile(path.join(source, 'library.sqlite'), 'database bytes');
    const corruptingFs = new Proxy(fs, {
      get(target, property) {
        if (property === 'rename') {
          return async (from, to) => {
            if (from === source && to === destination) {
              throw Object.assign(new Error('cross-device link'), { code: 'EXDEV' });
            }
            return target.rename(from, to);
          };
        }
        if (property === 'cp') {
          return async (from, to, options) => {
            await target.cp(from, to, options);
            const database = path.join(to, 'library.sqlite');
            if (fault === 'extra entry') {
              await target.writeFile(path.join(to, 'unexpected'), 'extra');
            } else if (fault === 'wrong entry type') {
              await target.rm(database);
              await target.mkdir(database);
            } else {
              const bytes = await target.readFile(database);
              bytes[0] ^= 0xff;
              await target.writeFile(database, bytes);
            }
          };
        }
        return target[property];
      },
    });
    const { service } = createOpenService(createNativeOpenStub(), {
      fs: corruptingFs,
      path,
      dialog: { showOpenDialog: async () => ({ canceled: false, filePaths: [destinationDirectory] }) },
    });

    await expect(service.relocateLibrary(source)).rejects.toThrow('verification failed');
    await expect(fs.readFile(path.join(source, 'library.sqlite'), 'utf8')).resolves.toBe('database bytes');
    await expect(fs.access(destination)).rejects.toMatchObject({ code: 'ENOENT' });
    await expect(fs.readdir(destinationDirectory)).resolves.toEqual([]);
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
});

test('a failed source cleanup keeps the verified cross-volume copy and reports the move', async () => {
  const root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'picto-cross-volume-cleanup-failure-')));
  const source = path.join(root, 'source', 'A.library');
  const destinationDirectory = path.join(root, 'destination');
  const destination = path.join(destinationDirectory, 'A.library');
  const warning = vi.spyOn(console, 'warn').mockImplementation(() => {});
  try {
    await fs.mkdir(source, { recursive: true });
    await fs.mkdir(destinationDirectory);
    await fs.writeFile(path.join(source, 'library.sqlite'), 'database bytes');
    const cleanupFailingFs = new Proxy(fs, {
      get(target, property) {
        if (property === 'rename') {
          return async (from, to) => {
            if (from === source && to === destination) {
              throw Object.assign(new Error('cross-device link'), { code: 'EXDEV' });
            }
            return target.rename(from, to);
          };
        }
        if (property === 'rm') {
          return async (targetPath, options) => {
            if (targetPath === source) throw Object.assign(new Error('resource busy'), { code: 'EBUSY' });
            return target.rm(targetPath, options);
          };
        }
        return target[property];
      },
    });
    const { service, state } = createOpenService(createNativeOpenStub(), {
      fs: cleanupFailingFs,
      path,
      dialog: { showOpenDialog: async () => ({ canceled: false, filePaths: [destinationDirectory] }) },
    });

    await expect(service.relocateLibrary(source)).resolves.toEqual({ action: 'relocated', newPath: destination });
    await expect(fs.readFile(path.join(source, 'library.sqlite'), 'utf8')).resolves.toBe('database bytes');
    await expect(fs.readFile(path.join(destination, 'library.sqlite'), 'utf8')).resolves.toBe('database bytes');
    expect(state.updates).toEqual([[source, destination]]);
    expect(warning).toHaveBeenCalledWith(
      '[library] unable to remove the original after a verified cross-volume copy',
      { oldPath: source, newPath: destination, message: 'resource busy' },
    );
  } finally {
    warning.mockRestore();
    await fs.rm(root, { recursive: true, force: true });
  }
});

test('opening a library installs standard Windows folder icon metadata', async () => {
  const writes = [];
  const attributes = [];
  const winPath = path.win32;
  const libraryPath = 'C:\\Libraries\\Main.library';
  const service = createLibraryHostService({
    fs: {
      readFile: async (filePath) => {
        if (filePath === 'C:\\Picto\\resources\\library-icons\\library.ico') return Buffer.from('icon');
        throw Object.assign(new Error('not found'), { code: 'ENOENT' });
      },
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
  expect(writes).toEqual([[iconPath, Buffer.from('icon')], [
    desktopIniPath,
    Buffer.from('[.ShellClassInfo]\r\nIconResource=.picto-library.ico,0\r\n'),
  ]]);
  expect(attributes).toEqual([
    ['attrib', ['+h', '+s', iconPath]],
    ['attrib', ['+h', '+s', desktopIniPath]],
    ['attrib', ['+r', libraryPath]],
  ]);
});

test.skipIf(process.platform !== 'win32')('Windows library reopen preserves hidden icons and refreshes changed bytes', async () => {
  const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-windows-icon-'));
  const libraryPath = path.join(directory, 'Main.library');
  const resources = path.join(directory, 'resources');
  const source = path.join(resources, 'library-icons', 'library.ico');
  const icon = path.join(libraryPath, '.picto-library.ico');
  const ini = path.join(libraryPath, 'desktop.ini');
  const run = promisify(execFile);
  const warning = vi.spyOn(console, 'warn').mockImplementation(() => {});
  const writes = [];
  let rejectIconWrite = false;
  const service = createLibraryHostService({
    fs: new Proxy(fs, { get(target, key) {
      if (key !== 'writeFile') return target[key];
      return async (filePath, ...args) => {
        if (filePath === icon && rejectIconWrite) throw Object.assign(new Error('blocked icon update'), { code: 'EACCES' });
        writes.push(filePath);
        return target.writeFile(filePath, ...args);
      };
    } }),
    path, dialog: {}, openLibrary: async () => {}, closeLibrary: async () => {},
    addLibraryToHistory: async () => {}, removeLibraryFromHistory: async () => {}, togglePinned: async () => {},
    getCachedConfig: () => ({}), saveGlobalConfig: async () => {}, updateLibraryPath: async () => {},
    getCurrentLibraryRoot: () => null, setCurrentLibraryRoot: () => {}, createMainWindow: () => {},
    sendToAllWindows: () => {}, buildAppMenu: () => {},
    platform: 'win32', resourcesPath: resources, isDefaultApp: false,
  });
  try {
    await fs.mkdir(libraryPath);
    await fs.mkdir(path.dirname(source), { recursive: true });
    const original = await fs.readFile(path.join(process.cwd(), 'build', 'library.ico'));
    await fs.writeFile(source, original);
    await service.switchLibrary(libraryPath);
    // Reproduce the old failure using actual Windows hidden/system attributes.
    await expect(fs.copyFile(source, icon)).rejects.toMatchObject({ code: 'EPERM' });
    writes.length = 0;
    await service.switchLibrary(libraryPath);
    expect(writes).toEqual([]);
    expect(await fs.readFile(icon)).toEqual(original);
    expect(warning).not.toHaveBeenCalled();

    const updated = Buffer.concat([original, Buffer.from('updated-icon')]);
    await fs.writeFile(source, updated);
    await service.switchLibrary(libraryPath);
    expect(writes).toEqual([icon]);
    expect(await fs.readFile(icon)).toEqual(updated);
    expect(warning).not.toHaveBeenCalled();

    rejectIconWrite = true;
    await fs.writeFile(source, original);
    await service.switchLibrary(libraryPath);
    expect(warning).toHaveBeenCalledWith('[library] unable to apply Windows folder icon', expect.objectContaining({ message: 'blocked icon update' }));
    expect(await fs.readFile(icon)).toEqual(updated);
    for (const filePath of [icon, ini]) {
      const { stdout } = await run('attrib', [filePath]);
      const attributes = stdout.slice(0, stdout.indexOf(filePath));
      expect(attributes).toContain('H');
      expect(attributes).toContain('S');
    }
  } finally {
    warning.mockRestore();
    for (const filePath of [icon, ini]) await run('attrib', ['-h', '-s', filePath]).catch(() => {});
    await run('attrib', ['-r', libraryPath]).catch(() => {});
    await fs.rm(directory, { recursive: true, force: true });
  }
});

test('opening a library applies the persistent macOS package icon', async () => {
  const icons = [];
  const libraryPath = '/Pictures/Main.library';
  const service = createLibraryHostService({
    fs: {},
    path: path.posix,
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
    path: path.posix,
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
    platform: 'linux',
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

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

/** Native opens stay pending until the test settles them, like the real addon. */
function createNativeOpenStub({ cancelOnClose = true } = {}) {
  const calls = [];
  const pending = [];
  const open = (call) => new Promise((resolve, reject) => {
    calls.push(call);
    pending.push({ path: call[1], resolve, reject });
  });
  const take = (libraryPath) => {
    const index = pending.findIndex((entry) => entry.path === libraryPath);
    if (index < 0) throw new Error(`${libraryPath} is not opening`);
    return pending.splice(index, 1)[0];
  };
  return {
    calls,
    openLibrary: (libraryPath) => open(['open', libraryPath]),
    openTutorialLibrary: (libraryPath, fixtureRoot) => open(['open-tutorial', libraryPath, fixtureRoot]),
    // `cloud.library.join` opens the joined database like an open does.
    joinCloudLibrary: (targetRoot) => open(['join', targetRoot]).then(() => JSON.stringify({ joined: targetRoot })),
    closeLibrary: async () => {
      calls.push(['close']);
      if (!cancelOnClose) return;
      for (const entry of pending.splice(0)) entry.reject(new Error(LIBRARY_OPEN_CANCELLED));
    },
    complete: (libraryPath) => take(libraryPath).resolve(),
    fail: (libraryPath, error) => take(libraryPath).reject(error),
    opening: () => pending.map((entry) => entry.path),
  };
}

function createOpenService(native, overrides = {}) {
  const events = [];
  const state = { current: null, histories: [], updates: [] };
  const service = createLibraryHostService({
    platform: 'linux',
    fs: {},
    path: path.posix,
    dialog: {},
    openLibrary: native.openLibrary,
    openTutorialLibrary: native.openTutorialLibrary,
    closeLibrary: native.closeLibrary,
    invokeSerialized: async (command, args) => {
      if (command === 'cloud.library.join') return native.joinCloudLibrary(args.target_root);
      return JSON.stringify({});
    },
    addLibraryToHistory: async (value) => { state.histories.push(value); },
    removeLibraryFromHistory: async () => {},
    togglePinned: async () => {},
    getCachedConfig: () => ({}),
    saveGlobalConfig: async () => {},
    updateLibraryPath: async (from, to) => { state.updates.push([from, to]); },
    getCurrentLibraryRoot: () => state.current,
    setCurrentLibraryRoot: (value) => { state.current = value; },
    createMainWindow: () => {},
    sendToAllWindows: (name, payload) => events.push([name, payload]),
    buildAppMenu: () => {},
    ...overrides,
  });
  return { service, events, state };
}

test('a switch superseded by a second switch publishes only the newer library', async () => {
  const native = createNativeOpenStub();
  const { service, events, state } = createOpenService(native);

  const first = service.switchLibrary('/Pictures/A.library');
  await flush();
  expect(native.opening()).toEqual(['/Pictures/A.library']);
  const second = service.switchLibrary('/Pictures/B.library');
  await flush();

  await expect(first).resolves.toBeUndefined();
  expect(native.opening()).toEqual(['/Pictures/B.library']);
  native.complete('/Pictures/B.library');
  await second;

  expect(events).toEqual([
    ['library-switching', { path: '/Pictures/A.library' }],
    ['library-switching', { path: '/Pictures/B.library' }],
    ['library-switched', { path: '/Pictures/B.library' }],
  ]);
  expect(state.current).toBe('/Pictures/B.library');
  expect(state.histories).toEqual(['/Pictures/B.library']);
  await expect(service.getLibraryConfig()).resolves.toMatchObject({ openingPath: null });
});

test('an open that completes after being superseded does not publish its library', async () => {
  const native = createNativeOpenStub({ cancelOnClose: false });
  const { service, events, state } = createOpenService(native);

  const first = service.switchLibrary('/Pictures/A.library');
  await flush();
  const second = service.switchLibrary('/Pictures/B.library');
  await flush();
  expect(native.opening()).toEqual(['/Pictures/A.library', '/Pictures/B.library']);

  native.complete('/Pictures/A.library');
  await expect(first).resolves.toBeUndefined();
  expect(state.current).toBeNull();
  expect(state.histories).toEqual([]);
  await expect(service.getLibraryConfig()).resolves.toMatchObject({ openingPath: '/Pictures/B.library' });

  native.complete('/Pictures/B.library');
  await second;
  expect(events).toEqual([
    ['library-switching', { path: '/Pictures/A.library' }],
    ['library-switching', { path: '/Pictures/B.library' }],
    ['library-switched', { path: '/Pictures/B.library' }],
  ]);
  expect(state.current).toBe('/Pictures/B.library');
});

test('re-clicking the library that is already opening leaves that open running', async () => {
  const native = createNativeOpenStub();
  const { service, events, state } = createOpenService(native);

  const first = service.switchLibrary('/Pictures/A.library');
  await flush();
  // A double-click during a migration must not restart the migration.
  await expect(service.switchLibrary('/Pictures/A.library')).resolves.toBeUndefined();
  expect(native.calls).toEqual([['close'], ['open', '/Pictures/A.library']]);
  expect(native.opening()).toEqual(['/Pictures/A.library']);
  await expect(service.getLibraryConfig()).resolves.toMatchObject({ openingPath: '/Pictures/A.library' });

  native.complete('/Pictures/A.library');
  await expect(first).resolves.toBeUndefined();
  expect(events).toEqual([
    ['library-switching', { path: '/Pictures/A.library' }],
    ['library-switched', { path: '/Pictures/A.library' }],
  ]);
  expect(state.current).toBe('/Pictures/A.library');
  await expect(service.getLibraryConfig()).resolves.toMatchObject({ openingPath: null });
});

test('a stale failure report for the closed library does not cancel the open in flight', async () => {
  const native = createNativeOpenStub();
  const { service, events, state } = createOpenService(native);
  state.current = '/Pictures/A.library';

  const switching = service.switchLibrary('/Pictures/B.library');
  await flush();
  // The Sidebar of A reports its initial read failing right after the switch closed A.
  await expect(service.failActiveLibrary('No library is open')).resolves.toBeNull();

  expect(native.opening()).toEqual(['/Pictures/B.library']);
  expect(native.calls.filter(([name]) => name === 'close')).toHaveLength(1);
  // Nothing is current between the close and the publish of B.
  expect(state.current).toBeNull();
  native.complete('/Pictures/B.library');
  await switching;

  expect(events).toEqual([
    ['library-switching', { path: '/Pictures/B.library' }],
    ['library-switched', { path: '/Pictures/B.library' }],
  ]);
  expect(state.current).toBe('/Pictures/B.library');
  await expect(service.getLibraryConfig()).resolves.toMatchObject({ openingPath: null, libraryFailure: null });
});

test('a failure report with no open in flight still closes the current library', async () => {
  const native = createNativeOpenStub();
  const { service, events, state } = createOpenService(native);
  state.current = '/Pictures/Broken.library';

  await expect(service.failActiveLibrary('database is unreadable')).resolves.toEqual({
    path: '/Pictures/Broken.library',
    message: 'database is unreadable',
  });

  expect(native.calls).toEqual([['close']]);
  expect(state.current).toBeNull();
  expect(events).toEqual([
    ['library-open-failed', { path: '/Pictures/Broken.library', message: 'database is unreadable' }],
  ]);
});

test('a guided tour open cancelled by shutdown stays silent and never re-opens the previous library', async () => {
  const native = createNativeOpenStub();
  const { service, events, state } = createOpenService(native, {
    fs: {
      readdir: async () => [],
      mkdtemp: async () => '/tmp/picto-guided-tour-test-1',
      mkdir: async () => {},
      rm: async () => {},
    },
    tutorialRoot: '/tmp',
    tutorialFixtureRoot: '/app/tutorial',
  });
  state.current = '/Pictures/Main.library';

  const starting = service.startTutorialLibrary();
  await flush();
  expect(native.opening()).toEqual(['/tmp/picto-guided-tour-test-1/Guided Tour.library']);
  service.markShuttingDown();
  await native.closeLibrary();

  await expect(starting).rejects.toThrow(LIBRARY_OPEN_CANCELLED);
  expect(service.isShuttingDown()).toBe(true);
  expect(native.calls.filter(([name]) => name === 'open')).toEqual([]);
  expect(events).toEqual([
    ['library-switching', { path: '/tmp/picto-guided-tour-test-1/Guided Tour.library', tutorial: true }],
  ]);
  expect(state.current).toBeNull();
});

test('a startup open cancelled by shutdown rethrows the sentinel without publishing a failure', async () => {
  const native = createNativeOpenStub();
  const { service, events, state } = createOpenService(native);

  const initializing = service.initializeInitialLibrary('/Pictures/Main.library');
  await flush();
  service.markShuttingDown();
  await native.closeLibrary();

  await expect(initializing).rejects.toThrow(LIBRARY_OPEN_CANCELLED);
  expect(events).toEqual([['library-switching', { path: '/Pictures/Main.library' }]]);
  expect(state.current).toBeNull();
  expect(state.histories).toEqual([]);
  await expect(service.getLibraryConfig()).resolves.toMatchObject({ openingPath: null });
});

test('an open cancelled by an explicit close reports the cancellation to the windows', async () => {
  const native = createNativeOpenStub();
  const { service, events } = createOpenService(native);

  const switching = service.switchLibrary('/Pictures/A.library');
  await flush();
  await native.closeLibrary();

  await expect(switching).rejects.toThrow(LIBRARY_OPEN_CANCELLED);
  expect(events).toEqual([
    ['library-switching', { path: '/Pictures/A.library' }],
    ['library-open-failed', { path: '/Pictures/A.library', message: LIBRARY_OPEN_CANCELLED, cancelled: true }],
  ]);
  await expect(service.getLibraryConfig()).resolves.toMatchObject({ openingPath: null });
});


function tutorialOverrides() {
  let sessions = 0;
  const removed = [];
  return {
    removed,
    overrides: {
      fs: {
        readdir: async () => [],
        mkdtemp: async () => `/tmp/picto-guided-tour-test-${++sessions}`,
        mkdir: async () => {},
        rm: async (...args) => { removed.push(args); },
      },
      tutorialRoot: '/tmp',
      tutorialFixtureRoot: '/app/tutorial',
    },
    sessionCount: () => sessions,
  };
}

function renameOverrides() {
  const renames = [];
  return {
    renames,
    overrides: {
      fs: {
        access: async () => { throw Object.assign(new Error('missing'), { code: 'ENOENT' }); },
        rename: async (...args) => { renames.push(args); },
      },
    },
  };
}

function cloudJoinOverrides() {
  const removed = [];
  // The database appears at the target once the native join downloaded it.
  let downloaded = false;
  return {
    removed,
    download: () => { downloaded = true; },
    overrides: {
      fs: {
        access: async (target) => {
          if (downloaded && target === '/Cloud/Shared.library/library.sqlite') return;
          throw Object.assign(new Error('missing'), { code: 'ENOENT' });
        },
        rm: async (...args) => { removed.push(args); },
      },
      dialog: { showOpenDialog: async () => ({ canceled: false, filePaths: ['/Cloud'] }) },
    },
    request: { provider: 'dropbox', accountLabel: 'Dropbox', rootPath: '/Dropbox', libraryId: 'lib-1', name: 'Shared' },
  };
}

test('any open failure while shutting down stays silent, sentinel or not', async () => {
  const native = createNativeOpenStub();
  const { service, events, state } = createOpenService(native);

  const initializing = service.initializeInitialLibrary('/Pictures/Main.library');
  await flush();
  service.markShuttingDown();
  // The shutdown close landed right after the open published, so the theme
  // read that follows the open fails instead of the open itself.
  native.fail('/Pictures/Main.library', new Error('No library is open'));

  await expect(initializing).rejects.toThrow('No library is open');
  expect(events).toEqual([['library-switching', { path: '/Pictures/Main.library' }]]);
  expect(state.current).toBeNull();
  await expect(service.getLibraryConfig()).resolves.toMatchObject({ openingPath: null });
});

test('a guided tour failing while shutting down never falls back to the previous library', async () => {
  const native = createNativeOpenStub();
  const tutorial = tutorialOverrides();
  const { service, events, state } = createOpenService(native, tutorial.overrides);
  state.current = '/Pictures/Main.library';

  const starting = service.startTutorialLibrary();
  await flush();
  service.markShuttingDown();
  native.fail('/tmp/picto-guided-tour-test-1/Guided Tour.library', new Error('No library is open'));

  await expect(starting).rejects.toThrow('No library is open');
  expect(native.calls.filter(([name]) => name === 'open')).toEqual([]);
  expect(events).toEqual([
    ['library-switching', { path: '/tmp/picto-guided-tour-test-1/Guided Tour.library', tutorial: true }],
  ]);
  expect(state.current).toBeNull();
});

test('no native open starts once the host is shutting down', async () => {
  const native = createNativeOpenStub();
  let quit = () => {};
  const { service, events, state } = createOpenService(native, {
    // Quit arrives while the switch is still waiting for its close.
    closeLibrary: async () => {
      await native.closeLibrary();
      quit();
    },
  });
  quit = () => service.markShuttingDown();

  await expect(service.switchLibrary('/Pictures/A.library')).rejects.toThrow(LIBRARY_OPEN_CANCELLED);
  expect(native.calls).toEqual([['close']]);

  // A startup that reaches its open after the shutdown close is refused as well.
  await expect(service.initializeInitialLibrary('/Pictures/B.library')).rejects.toThrow(LIBRARY_OPEN_CANCELLED);
  expect(native.calls.filter(([name]) => name === 'open')).toEqual([]);
  expect(events.map(([name]) => name)).toEqual(['library-switching', 'library-switching']);
  expect(state.current).toBeNull();
  expect(state.histories).toEqual([]);
});

test('a cancelled cloud join keeps and lists the downloaded library', async () => {
  const native = createNativeOpenStub();
  const join = cloudJoinOverrides();
  const saved = [];
  const { service, events, state } = createOpenService(native, {
    ...join.overrides,
    getCachedConfig: () => ({ libraryMeta: {} }),
    saveGlobalConfig: async (config) => { saved.push(JSON.parse(JSON.stringify(config))); },
  });
  state.current = '/Pictures/Main.library';

  const joining = service.joinCloudLibrary(join.request);
  await flush();
  expect(native.opening()).toEqual(['/Cloud/Shared.library']);
  // A close can only cancel the join's final open: the database is complete
  // at the target by then.
  join.download();
  await native.closeLibrary();

  await expect(joining).rejects.toThrow(LIBRARY_OPEN_CANCELLED);
  expect(join.removed).toEqual([]);
  // Listed, so that it opens from Library Manager instead of refusing every
  // later join to its folder; not published.
  expect(state.histories).toEqual(['/Cloud/Shared.library']);
  expect(saved.at(-1).libraryMeta['/Cloud/Shared.library']).toEqual({ cloudLibraryId: 'lib-1' });
  expect(events.filter(([name]) => name !== 'library-meta-changed')).toEqual([
    ['library-switching', { path: '/Cloud/Shared.library' }],
    ['library-open-failed', { path: '/Cloud/Shared.library', message: LIBRARY_OPEN_CANCELLED, cancelled: true }],
  ]);
  // The native join closed the previous library and a cancelled open leaves
  // nothing open: the previous library must be openable again.
  expect(state.current).toBeNull();
});

test('a cloud join superseded by a switch keeps and lists the downloaded library', async () => {
  const native = createNativeOpenStub();
  const join = cloudJoinOverrides();
  const { service, events, state } = createOpenService(native, join.overrides);

  const joining = service.joinCloudLibrary(join.request);
  await flush();
  join.download();
  const switching = service.switchLibrary('/Pictures/B.library');
  await flush();

  await expect(joining).resolves.toEqual({ library_id: 'lib-1', path: '/Cloud/Shared.library', opened: false });
  expect(join.removed).toEqual([]);
  expect(state.histories).toEqual(['/Cloud/Shared.library']);
  native.complete('/Pictures/B.library');
  await switching;
  expect(state.current).toBe('/Pictures/B.library');
  expect(state.histories).toEqual(['/Cloud/Shared.library', '/Pictures/B.library']);
  expect(events.filter(([name]) => name !== 'library-meta-changed')).toEqual([
    ['library-switching', { path: '/Cloud/Shared.library' }],
    ['library-switching', { path: '/Pictures/B.library' }],
    ['library-switched', { path: '/Pictures/B.library' }],
  ]);
});

test('a cloud join interrupted by shutdown keeps and lists the downloaded library and stays silent', async () => {
  const native = createNativeOpenStub();
  const join = cloudJoinOverrides();
  const { service, events, state } = createOpenService(native, join.overrides);

  const joining = service.joinCloudLibrary(join.request);
  await flush();
  join.download();
  service.markShuttingDown();
  await native.closeLibrary();

  await expect(joining).rejects.toThrow(LIBRARY_OPEN_CANCELLED);
  expect(join.removed).toEqual([]);
  expect(state.histories).toEqual(['/Cloud/Shared.library']);
  expect(events.filter(([name]) => name !== 'library-meta-changed')).toEqual([
    ['library-switching', { path: '/Cloud/Shared.library' }],
  ]);
});

test('a cloud join superseded before its database exists removes its remnants', async () => {
  const native = createNativeOpenStub();
  const join = cloudJoinOverrides();
  const { service, events, state } = createOpenService(native, join.overrides);

  const joining = service.joinCloudLibrary(join.request);
  await flush();
  const switching = service.switchLibrary('/Pictures/B.library');
  await flush();

  // Nothing valid to list: the staging remnants would block every later
  // join to that folder.
  await expect(joining).resolves.toBeNull();
  expect(join.removed).toEqual([['/Cloud/Shared.library', { recursive: true, force: true }]]);
  native.complete('/Pictures/B.library');
  await switching;
  expect(state.current).toBe('/Pictures/B.library');
  expect(state.histories).toEqual(['/Pictures/B.library']);
  expect(events).toEqual([
    ['library-switching', { path: '/Cloud/Shared.library' }],
    ['library-switching', { path: '/Pictures/B.library' }],
    ['library-switched', { path: '/Pictures/B.library' }],
  ]);
});

test('a cloud join failing natively underneath a takeover removes its remnants silently', async () => {
  const native = createNativeOpenStub({ cancelOnClose: false });
  const join = cloudJoinOverrides();
  const { service, events, state } = createOpenService(native, join.overrides);
  state.current = '/Pictures/Main.library';

  const joining = service.joinCloudLibrary(join.request);
  await flush();
  const switching = service.switchLibrary('/Pictures/B.library');
  await flush();
  // The join's download fails after the switch took over.
  native.fail('/Cloud/Shared.library', new Error('download failed'));

  await expect(joining).resolves.toBeNull();
  expect(join.removed).toEqual([['/Cloud/Shared.library', { recursive: true, force: true }]]);
  expect(state.histories).toEqual([]);
  native.complete('/Pictures/B.library');
  await switching;
  expect(state.current).toBe('/Pictures/B.library');
  expect(events).toEqual([
    ['library-switching', { path: '/Cloud/Shared.library' }],
    ['library-switching', { path: '/Pictures/B.library' }],
    ['library-switched', { path: '/Pictures/B.library' }],
  ]);
});

test('a failed cloud join removes its remnants and returns to the previous library', async () => {
  const native = createNativeOpenStub();
  const join = cloudJoinOverrides();
  const { service, events, state } = createOpenService(native, join.overrides);
  state.current = '/Pictures/Main.library';

  const joining = service.joinCloudLibrary(join.request);
  await flush();
  native.fail('/Cloud/Shared.library', new Error('download failed'));

  await expect(joining).rejects.toThrow('download failed');
  expect(join.removed).toEqual([['/Cloud/Shared.library', { recursive: true, force: true }]]);
  expect(events).toEqual([
    ['library-switching', { path: '/Cloud/Shared.library' }],
    ['library-open-failed', { path: '/Cloud/Shared.library', message: 'download failed' }],
    ['library-switched', { path: '/Pictures/Main.library' }],
  ]);
});

test('a cancelled rename leaves nothing current and reports the cancellation', async () => {
  const native = createNativeOpenStub();
  const rename = renameOverrides();
  const { service, events, state } = createOpenService(native, rename.overrides);
  state.current = '/Pictures/A.library';

  const renaming = service.renameLibrary('/Pictures/A.library', 'B');
  await flush();
  await expect(service.getLibraryConfig()).resolves.toMatchObject({
    currentPath: null,
    openingPath: '/Pictures/B.library',
  });
  await native.closeLibrary();

  await expect(renaming).rejects.toThrow(LIBRARY_OPEN_CANCELLED);
  expect(rename.renames).toEqual([['/Pictures/A.library', '/Pictures/B.library']]);
  expect(state.updates).toEqual([['/Pictures/A.library', '/Pictures/B.library']]);
  expect(state.current).toBeNull();
  expect(events).toEqual([
    ['library-switching', { path: '/Pictures/B.library' }],
    ['library-open-failed', { path: '/Pictures/B.library', message: LIBRARY_OPEN_CANCELLED, cancelled: true }],
  ]);
  await expect(service.getLibraryConfig()).resolves.toMatchObject({ currentPath: null, openingPath: null });
});

test('a rename whose open fails leaves nothing current and reports the failure', async () => {
  const native = createNativeOpenStub();
  const rename = renameOverrides();
  const { service, events, state } = createOpenService(native, rename.overrides);
  state.current = '/Pictures/A.library';

  const renaming = service.renameLibrary('/Pictures/A.library', 'B');
  await flush();
  native.fail('/Pictures/B.library', new Error('database is locked'));

  await expect(renaming).rejects.toThrow('database is locked');
  expect(state.current).toBeNull();
  expect(events).toEqual([
    ['library-switching', { path: '/Pictures/B.library' }],
    ['library-open-failed', { path: '/Pictures/B.library', message: 'database is locked' }],
  ]);
  await expect(service.getLibraryConfig()).resolves.toMatchObject({ currentPath: null, openingPath: null });
});

test('a rename interrupted by shutdown stays silent', async () => {
  const native = createNativeOpenStub();
  const rename = renameOverrides();
  const { service, events, state } = createOpenService(native, rename.overrides);
  state.current = '/Pictures/A.library';

  const renaming = service.renameLibrary('/Pictures/A.library', 'B');
  await flush();
  service.markShuttingDown();
  await native.closeLibrary();

  await expect(renaming).rejects.toThrow(LIBRARY_OPEN_CANCELLED);
  expect(state.current).toBeNull();
  expect(events).toEqual([['library-switching', { path: '/Pictures/B.library' }]]);
});

test('a rename superseded by a switch leaves the switched library current', async () => {
  const native = createNativeOpenStub();
  const rename = renameOverrides();
  const { service, events, state } = createOpenService(native, rename.overrides);
  state.current = '/Pictures/A.library';

  const renaming = service.renameLibrary('/Pictures/A.library', 'B');
  await flush();
  const switching = service.switchLibrary('/Pictures/C.library');
  await flush();

  await expect(renaming).resolves.toEqual({ newPath: '/Pictures/B.library' });
  native.complete('/Pictures/C.library');
  await switching;
  expect(state.current).toBe('/Pictures/C.library');
  expect(events).toEqual([
    ['library-switching', { path: '/Pictures/B.library' }],
    ['library-switching', { path: '/Pictures/C.library' }],
    ['library-switched', { path: '/Pictures/C.library' }],
  ]);
});

test('a failed rename re-opens the library under its old name', async () => {
  const native = createNativeOpenStub();
  const { service, events, state } = createOpenService(native, {
    fs: {
      access: async () => { throw Object.assign(new Error('missing'), { code: 'ENOENT' }); },
      rename: async () => { throw new Error('EPERM'); },
    },
  });
  state.current = '/Pictures/A.library';

  const renaming = service.renameLibrary('/Pictures/A.library', 'B');
  await flush();
  expect(native.opening()).toEqual(['/Pictures/A.library']);
  expect(state.current).toBeNull();
  native.complete('/Pictures/A.library');

  await expect(renaming).rejects.toThrow('Failed to rename library: EPERM');
  expect(state.current).toBe('/Pictures/A.library');
  expect(events).toEqual([
    ['library-switching', { path: '/Pictures/B.library' }],
    ['library-switched', { path: '/Pictures/A.library' }],
  ]);
  await expect(service.getLibraryConfig()).resolves.toMatchObject({ openingPath: null });
});

test('a cancelled relocation leaves nothing current and reports the cancellation', async () => {
  const native = createNativeOpenStub();
  const renames = [];
  const { service, events, state } = createOpenService(native, {
    fs: {
      realpath: async (value) => value,
      access: async (target) => {
        if (target !== '/Pictures/A.library') throw Object.assign(new Error('missing'), { code: 'ENOENT' });
      },
      rename: async (...args) => { renames.push(args); },
    },
    dialog: { showOpenDialog: async () => ({ canceled: false, filePaths: ['/Archive'] }) },
  });
  state.current = '/Pictures/A.library';

  const relocating = service.relocateLibrary('/Pictures/A.library');
  await flush();
  await expect(service.getLibraryConfig()).resolves.toMatchObject({
    currentPath: null,
    openingPath: '/Archive/A.library',
  });
  await native.closeLibrary();

  await expect(relocating).rejects.toThrow(LIBRARY_OPEN_CANCELLED);
  expect(renames).toEqual([['/Pictures/A.library', '/Archive/A.library']]);
  expect(state.updates).toEqual([['/Pictures/A.library', '/Archive/A.library']]);
  expect(state.current).toBeNull();
  expect(events).toEqual([
    ['library-switching', { path: '/Archive/A.library' }],
    ['library-open-failed', { path: '/Archive/A.library', message: LIBRARY_OPEN_CANCELLED, cancelled: true }],
  ]);
  await expect(service.getLibraryConfig()).resolves.toMatchObject({ currentPath: null, openingPath: null });
});

test('a cancelled guided-tour finish leaves nothing current and ends the session', async () => {
  const native = createNativeOpenStub();
  const tutorial = tutorialOverrides();
  const { service, events, state } = createOpenService(native, tutorial.overrides);
  state.current = '/Pictures/Main.library';
  const tourPath = '/tmp/picto-guided-tour-test-1/Guided Tour.library';

  const starting = service.startTutorialLibrary();
  await flush();
  native.complete(tourPath);
  await expect(starting).resolves.toEqual({ path: tourPath });

  const finishing = service.finishTutorialLibrary();
  await flush();
  // The tour is over as soon as the finish starts.
  expect(service.getTutorialSession()).toEqual({ active: false });
  expect(state.current).toBeNull();
  await native.closeLibrary();

  await expect(finishing).rejects.toThrow(LIBRARY_OPEN_CANCELLED);
  expect(service.getTutorialSession()).toEqual({ active: false });
  expect(state.current).toBeNull();
  expect(tutorial.removed).toContainEqual(['/tmp/picto-guided-tour-test-1', { recursive: true, force: true }]);
  expect(events.slice(-2)).toEqual([
    ['library-switching', { path: '/Pictures/Main.library', tutorial: false }],
    ['library-open-failed', { path: '/Pictures/Main.library', message: LIBRARY_OPEN_CANCELLED, cancelled: true }],
  ]);
  await expect(service.getLibraryConfig()).resolves.toMatchObject({ currentPath: null, openingPath: null });
});

test('a tour started while the finish is still opening supersedes it and returns to the user library', async () => {
  const native = createNativeOpenStub();
  const tutorial = tutorialOverrides();
  const { service, events, state } = createOpenService(native, tutorial.overrides);
  state.current = '/Pictures/Main.library';
  const firstTour = '/tmp/picto-guided-tour-test-1/Guided Tour.library';
  const secondTour = '/tmp/picto-guided-tour-test-2/Guided Tour.library';

  const starting = service.startTutorialLibrary();
  await flush();
  native.complete(firstTour);
  await starting;

  const finishing = service.finishTutorialLibrary();
  await flush();
  expect(native.opening()).toEqual(['/Pictures/Main.library']);
  // The new tour cancels the restore and starts its own session.
  const restarting = service.startTutorialLibrary();
  await flush();
  await expect(finishing).resolves.toEqual({ restored: false, path: '/Pictures/Main.library' });
  expect(tutorial.sessionCount()).toBe(2);
  expect(native.opening()).toEqual([secondTour]);
  // The superseded finish touched nothing: its directory waits for the next cleanup.
  expect(tutorial.removed).toEqual([]);
  native.complete(secondTour);
  await expect(restarting).resolves.toEqual({ path: secondTour });
  expect(service.getTutorialSession()).toEqual({ active: true, path: secondTour });
  expect(state.current).toBe(secondTour);

  // Finishing the new tour restores the user's library, not the first tour directory.
  const finishingAgain = service.finishTutorialLibrary();
  await flush();
  expect(native.opening()).toEqual(['/Pictures/Main.library']);
  native.complete('/Pictures/Main.library');
  await expect(finishingAgain).resolves.toEqual({ restored: true, path: '/Pictures/Main.library' });
  expect(state.current).toBe('/Pictures/Main.library');
  expect(tutorial.removed).toEqual([['/tmp/picto-guided-tour-test-2', { recursive: true, force: true }]]);
  expect(events.filter(([name]) => name === 'library-switched')).toEqual([
    ['library-switched', { path: firstTour, tutorial: true }],
    ['library-switched', { path: secondTour, tutorial: true }],
    ['library-switched', { path: '/Pictures/Main.library', restored: true }],
  ]);
});

test('switching during a guided-tour finish is allowed and supersedes the restore', async () => {
  const native = createNativeOpenStub();
  const tutorial = tutorialOverrides();
  const { service, events, state } = createOpenService(native, tutorial.overrides);
  state.current = '/Pictures/Main.library';
  const tourPath = '/tmp/picto-guided-tour-test-1/Guided Tour.library';

  const starting = service.startTutorialLibrary();
  await flush();
  native.complete(tourPath);
  await starting;
  await expect(service.switchLibrary('/Pictures/B.library')).rejects.toThrow('Exit the guided tour');

  const finishing = service.finishTutorialLibrary();
  await flush();
  expect(native.opening()).toEqual(['/Pictures/Main.library']);
  const switching = service.switchLibrary('/Pictures/B.library');
  await flush();

  await expect(finishing).resolves.toEqual({ restored: false, path: '/Pictures/Main.library' });
  expect(native.opening()).toEqual(['/Pictures/B.library']);
  expect(tutorial.removed).toEqual([]);
  native.complete('/Pictures/B.library');
  await switching;
  expect(state.current).toBe('/Pictures/B.library');
  expect(service.getTutorialSession()).toEqual({ active: false });
  expect(events.slice(-3)).toEqual([
    ['library-switching', { path: '/Pictures/Main.library', tutorial: false }],
    ['library-switching', { path: '/Pictures/B.library' }],
    ['library-switched', { path: '/Pictures/B.library' }],
  ]);
});

test('a guided-tour finish publishes the previous library before removing the session directory', async () => {
  const native = createNativeOpenStub();
  const tutorial = tutorialOverrides();
  const warning = vi.spyOn(console, 'warn').mockImplementation(() => {});
  const { service, events, state } = createOpenService(native, {
    ...tutorial.overrides,
    fs: {
      ...tutorial.overrides.fs,
      rm: async (target) => {
        if (target === '/tmp/picto-guided-tour-test-1') {
          throw Object.assign(new Error('resource busy'), { code: 'EBUSY' });
        }
      },
    },
  });
  state.current = '/Pictures/Main.library';
  const tourPath = '/tmp/picto-guided-tour-test-1/Guided Tour.library';

  try {
    const starting = service.startTutorialLibrary();
    await flush();
    native.complete(tourPath);
    await starting;

    const finishing = service.finishTutorialLibrary();
    await flush();
    native.complete('/Pictures/Main.library');
    await expect(finishing).resolves.toEqual({ restored: true, path: '/Pictures/Main.library' });
    expect(state.current).toBe('/Pictures/Main.library');
    expect(events.at(-1)).toEqual(['library-switched', { path: '/Pictures/Main.library', restored: true }]);
    await expect(service.getLibraryConfig()).resolves.toMatchObject({ openingPath: null });
    expect(warning).toHaveBeenCalledWith(
      '[library] unable to remove the guided-tour session directory',
      { sessionPath: '/tmp/picto-guided-tour-test-1', message: 'resource busy' },
    );
  } finally {
    warning.mockRestore();
  }
});

test('a superseded guided-tour fallback never publishes or overwrites the current root', async () => {
  const native = createNativeOpenStub();
  const tutorial = tutorialOverrides();
  const { service, events, state } = createOpenService(native, tutorial.overrides);
  state.current = '/Pictures/Main.library';
  const tourPath = '/tmp/picto-guided-tour-test-1/Guided Tour.library';

  const starting = service.startTutorialLibrary();
  await flush();
  native.fail(tourPath, new Error('fixture missing'));
  await flush();
  // The failed tour falls back to re-opening the previous library...
  expect(native.opening()).toEqual(['/Pictures/Main.library']);
  expect(service.getTutorialSession()).toEqual({ active: false });
  await expect(service.getLibraryConfig()).resolves.toMatchObject({ openingPath: '/Pictures/Main.library' });

  // ...and a switch issued meanwhile takes over. The tour never opened, so
  // the renderer must not treat it as started.
  const startRejection = expect(starting).rejects.toThrow(LIBRARY_OPEN_CANCELLED);
  const switching = service.switchLibrary('/Pictures/B.library');
  await flush();
  await startRejection;
  native.complete('/Pictures/B.library');
  await switching;

  expect(state.current).toBe('/Pictures/B.library');
  expect(events).toEqual([
    ['library-switching', { path: tourPath, tutorial: true }],
    ['library-switching', { path: '/Pictures/B.library' }],
    ['library-switched', { path: '/Pictures/B.library' }],
  ]);
});

test('tour seeding stops at the first command after a takeover', async () => {
  const native = createNativeOpenStub();
  const tutorial = tutorialOverrides();
  const commands = [];
  let releaseSeed = () => {};
  const { service, state } = createOpenService(native, {
    ...tutorial.overrides,
    invokeSerialized: async (command) => {
      commands.push(command);
      if (commands.length === 1) await new Promise((resolve) => { releaseSeed = resolve; });
      return JSON.stringify({ folder_id: 1, smart_folder_id: 1, subscription_id: 1 });
    },
  });
  state.current = '/Pictures/Main.library';
  const tourPath = '/tmp/picto-guided-tour-test-1/Guided Tour.library';

  const starting = service.startTutorialLibrary();
  await flush();
  native.complete(tourPath);
  await flush();
  // The first seed command is in flight when a takeover that ignores the
  // tour session (like a cloud join) begins.
  expect(commands).toEqual(['folders.create']);
  const initializing = service.initializeInitialLibrary('/Pictures/B.library');
  await flush();
  releaseSeed();

  await expect(starting).rejects.toThrow(LIBRARY_OPEN_CANCELLED);
  // No further seed command was issued after the takeover.
  expect(commands).toEqual(['folders.create']);
  expect(service.getTutorialSession()).toEqual({ active: false });
  native.complete('/Pictures/B.library');
  await initializing;
  expect(state.current).toBe('/Pictures/B.library');
});

test('a superseded guided-tour start rejects with the cancellation sentinel and ends its session', async () => {
  const native = createNativeOpenStub();
  const tutorial = tutorialOverrides();
  const { service, events, state } = createOpenService(native, tutorial.overrides);
  state.current = '/Pictures/Main.library';
  const tourPath = '/tmp/picto-guided-tour-test-1/Guided Tour.library';

  const starting = service.startTutorialLibrary();
  await flush();
  expect(native.opening()).toEqual([tourPath]);
  expect(service.getTutorialSession()).toEqual({ active: true, path: tourPath });
  // Only an entry point that ignores the tour session can take over here; it
  // does not close, so the tour's open completes underneath the takeover.
  const initializing = service.initializeInitialLibrary('/Pictures/B.library');
  await flush();
  native.complete(tourPath);

  await expect(starting).rejects.toThrow(LIBRARY_OPEN_CANCELLED);
  expect(service.getTutorialSession()).toEqual({ active: false });
  expect(tutorial.removed).toEqual([['/tmp/picto-guided-tour-test-1', { recursive: true, force: true }]]);
  expect(native.opening()).toEqual(['/Pictures/B.library']);
  native.complete('/Pictures/B.library');
  await initializing;
  expect(state.current).toBe('/Pictures/B.library');
  expect(events).toEqual([
    ['library-switching', { path: tourPath, tutorial: true }],
    ['library-switching', { path: '/Pictures/B.library' }],
    ['library-switched', { path: '/Pictures/B.library' }],
  ]);
});

test('a guided-tour reset superseded by the finish rejects with the cancellation sentinel', async () => {
  const native = createNativeOpenStub();
  const tutorial = tutorialOverrides();
  const { service, events, state } = createOpenService(native, tutorial.overrides);
  state.current = '/Pictures/Main.library';
  const firstTour = '/tmp/picto-guided-tour-test-1/Guided Tour.library';
  const secondTour = '/tmp/picto-guided-tour-test-2/Guided Tour.library';

  const starting = service.startTutorialLibrary();
  await flush();
  native.complete(firstTour);
  await starting;

  const resetting = service.resetTutorialLibrary();
  const resetRejection = expect(resetting).rejects.toThrow(LIBRARY_OPEN_CANCELLED);
  await flush();
  expect(native.opening()).toEqual([secondTour]);
  const finishing = service.finishTutorialLibrary();
  await flush();

  await resetRejection;
  expect(service.getTutorialSession()).toEqual({ active: false });
  expect(native.opening()).toEqual(['/Pictures/Main.library']);
  native.complete('/Pictures/Main.library');
  await expect(finishing).resolves.toEqual({ restored: true, path: '/Pictures/Main.library' });
  expect(state.current).toBe('/Pictures/Main.library');
  expect(tutorial.removed).toContainEqual(['/tmp/picto-guided-tour-test-1', { recursive: true, force: true }]);
  expect(tutorial.removed).toContainEqual(['/tmp/picto-guided-tour-test-2', { recursive: true, force: true }]);
  expect(events.at(-1)).toEqual(['library-switched', { path: '/Pictures/Main.library', restored: true }]);
});

test('a superseded switch never closes the library the newer switch is opening', async () => {
  const native = createNativeOpenStub();
  let releaseIcon = () => {};
  const { service, events, state } = createOpenService(native, {
    platform: 'darwin',
    resourcesPath: '/Applications/Picto.app/Contents/Resources',
    isDefaultApp: false,
    // The icon step of A stays pending until the test releases it.
    setFileIcon: (_icon, libraryPath) => (libraryPath === '/Pictures/A.library'
      ? new Promise((resolve) => { releaseIcon = () => resolve(true); })
      : Promise.resolve(true)),
  });

  const first = service.switchLibrary('/Pictures/A.library');
  await flush();
  const second = service.switchLibrary('/Pictures/B.library');
  await flush();
  expect(native.calls).toEqual([['close'], ['open', '/Pictures/B.library']]);

  // A's icon step settles after B started opening: A must neither close
  // (that would cancel B) nor open.
  releaseIcon();
  await expect(first).resolves.toBeUndefined();
  expect(native.calls).toEqual([['close'], ['open', '/Pictures/B.library']]);
  expect(native.opening()).toEqual(['/Pictures/B.library']);

  native.complete('/Pictures/B.library');
  await second;
  expect(state.current).toBe('/Pictures/B.library');
  expect(events).toEqual([
    ['library-switching', { path: '/Pictures/A.library' }],
    ['library-switching', { path: '/Pictures/B.library' }],
    ['library-switched', { path: '/Pictures/B.library' }],
  ]);
});

test('a rename superseded before its disk rename renames nothing and reports the takeover', async () => {
  const native = createNativeOpenStub();
  const renames = [];
  const pendingCloses = [];
  const { service, state } = createOpenService(native, {
    fs: {
      access: async () => { throw Object.assign(new Error('missing'), { code: 'ENOENT' }); },
      rename: async (...args) => { renames.push(args); },
    },
    closeLibrary: async () => {
      await native.closeLibrary();
      await new Promise((resolve) => pendingCloses.push(resolve));
    },
  });
  state.current = '/Pictures/A.library';

  const renaming = service.renameLibrary('/Pictures/A.library', 'B');
  await flush();
  const switching = service.switchLibrary('/Pictures/C.library');
  await flush();
  expect(pendingCloses).toHaveLength(2);
  // The rename's close settles after the switch took over.
  pendingCloses.shift()();

  await expect(renaming).rejects.toThrow(LIBRARY_OPEN_CANCELLED);
  expect(renames).toEqual([]);
  expect(state.updates).toEqual([]);

  pendingCloses.shift()();
  await flush();
  native.complete('/Pictures/C.library');
  await switching;
  expect(state.current).toBe('/Pictures/C.library');
});

test('a close failing before the open leaves no open in flight and lets the switch be retried', async () => {
  const native = createNativeOpenStub();
  let failClose = true;
  const { service, events, state } = createOpenService(native, {
    closeLibrary: async () => {
      if (failClose) throw new Error('checkpoint failed');
      await native.closeLibrary();
    },
  });
  state.current = '/Pictures/A.library';

  await expect(service.switchLibrary('/Pictures/B.library')).rejects.toThrow('checkpoint failed');
  expect(events).toEqual([
    ['library-switching', { path: '/Pictures/B.library' }],
    ['library-open-failed', { path: '/Pictures/B.library', message: 'checkpoint failed' }],
  ]);
  await expect(service.getLibraryConfig()).resolves.toMatchObject({ currentPath: null, openingPath: null });

  // The re-click guard depends on an open in flight, not on a stale path.
  failClose = false;
  const retrying = service.switchLibrary('/Pictures/B.library');
  await flush();
  expect(native.opening()).toEqual(['/Pictures/B.library']);
  native.complete('/Pictures/B.library');
  await expect(retrying).resolves.toBeUndefined();
  expect(state.current).toBe('/Pictures/B.library');
});

test('a rename whose history update fails still opens and publishes the renamed library', async () => {
  const native = createNativeOpenStub();
  const rename = renameOverrides();
  const warnings = [];
  const warn = vi.spyOn(console, 'warn').mockImplementation((...args) => { warnings.push(args); });
  const { service, events, state } = createOpenService(native, {
    ...rename.overrides,
    updateLibraryPath: async () => { throw new Error('config is read-only'); },
  });
  state.current = '/Pictures/A.library';

  const renaming = service.renameLibrary('/Pictures/A.library', 'B');
  await flush();
  expect(rename.renames).toEqual([['/Pictures/A.library', '/Pictures/B.library']]);
  expect(native.opening()).toEqual(['/Pictures/B.library']);
  native.complete('/Pictures/B.library');

  await expect(renaming).resolves.toEqual({ newPath: '/Pictures/B.library' });
  expect(events).toEqual([
    ['library-switching', { path: '/Pictures/B.library' }],
    ['library-switched', { path: '/Pictures/B.library' }],
  ]);
  expect(state.current).toBe('/Pictures/B.library');
  expect(warnings.some(([message]) => String(message).includes('bookkeeping'))).toBe(true);
  warn.mockRestore();
});

test('a failure report while no library is current is ignored', async () => {
  const native = createNativeOpenStub();
  const { service, events, state } = createOpenService(native);
  state.current = '/Pictures/A.library';

  const switching = service.switchLibrary('/Pictures/B.library');
  await flush();
  await native.closeLibrary();
  await expect(switching).rejects.toThrow(LIBRARY_OPEN_CANCELLED);
  expect(state.current).toBeNull();
  events.length = 0;

  // A late report from a window of the closed library A changes nothing:
  // no second close, no second failure, no chooser.
  await expect(service.failActiveLibrary('No library is open')).resolves.toBeNull();
  expect(native.calls.filter(([name]) => name === 'close')).toHaveLength(2);
  expect(events).toEqual([]);
  await expect(service.getLibraryConfig()).resolves.toMatchObject({ currentPath: null, libraryFailure: null });
});

test('a cloud join that completed before being superseded is listed without being published', async () => {
  const native = createNativeOpenStub({ cancelOnClose: false });
  const join = cloudJoinOverrides();
  const saved = [];
  const { service, events, state } = createOpenService(native, {
    ...join.overrides,
    getCachedConfig: () => ({ libraryMeta: {} }),
    saveGlobalConfig: async (config) => { saved.push(JSON.parse(JSON.stringify(config))); },
  });
  state.current = '/Pictures/Main.library';

  const joining = service.joinCloudLibrary(join.request);
  await flush();
  const switching = service.switchLibrary('/Pictures/B.library');
  await flush();
  // The switch's close waits for the join, which completes natively first.
  native.complete('/Cloud/Shared.library');

  await expect(joining).resolves.toEqual({ joined: '/Cloud/Shared.library', opened: false });
  expect(state.histories).toEqual(['/Cloud/Shared.library']);
  expect(saved.at(-1).libraryMeta['/Cloud/Shared.library']).toEqual({ cloudLibraryId: 'lib-1' });
  expect(join.removed).toEqual([]);
  native.complete('/Pictures/B.library');
  await switching;
  expect(state.current).toBe('/Pictures/B.library');
  expect(state.histories).toEqual(['/Cloud/Shared.library', '/Pictures/B.library']);
  expect(events.filter(([name]) => name === 'library-switched')).toEqual([
    ['library-switched', { path: '/Pictures/B.library' }],
  ]);
});

test('a failed guided tour re-opens the previous library and reports the tour error', async () => {
  const native = createNativeOpenStub();
  const tutorial = tutorialOverrides();
  const { service, events, state } = createOpenService(native, tutorial.overrides);
  state.current = '/Pictures/Main.library';
  const tourPath = '/tmp/picto-guided-tour-test-1/Guided Tour.library';

  const starting = service.startTutorialLibrary();
  await flush();
  native.fail(tourPath, new Error('fixture missing'));
  await flush();
  native.complete('/Pictures/Main.library');

  await expect(starting).rejects.toThrow('fixture missing');
  expect(state.current).toBe('/Pictures/Main.library');
  expect(events).toEqual([
    ['library-switching', { path: tourPath, tutorial: true }],
    ['library-switched', { path: '/Pictures/Main.library', restored: true }],
  ]);
  await expect(service.getLibraryConfig()).resolves.toMatchObject({ openingPath: null });
});

function failingHistory() {
  return {
    addLibraryToHistory: async () => { throw new Error('config is read-only'); },
  };
}

test('a switch whose history write fails still publishes the opened library', async () => {
  const native = createNativeOpenStub();
  const warning = vi.spyOn(console, 'warn').mockImplementation(() => {});
  const { service, events, state } = createOpenService(native, failingHistory());
  try {
    const switching = service.switchLibrary('/Pictures/A.library');
    await flush();
    native.complete('/Pictures/A.library');

    await expect(switching).resolves.toBeUndefined();
    expect(state.current).toBe('/Pictures/A.library');
    expect(events).toEqual([
      ['library-switching', { path: '/Pictures/A.library' }],
      ['library-switched', { path: '/Pictures/A.library' }],
    ]);
    await expect(service.getLibraryConfig()).resolves.toMatchObject({
      currentPath: '/Pictures/A.library',
      openingPath: null,
    });
    expect(warning).toHaveBeenCalledWith('[library] library bookkeeping failed', {
      libraryPath: '/Pictures/A.library',
      message: 'config is read-only',
    });
  } finally {
    warning.mockRestore();
  }
});

test('a startup open whose history write fails still publishes the library', async () => {
  const native = createNativeOpenStub();
  const warning = vi.spyOn(console, 'warn').mockImplementation(() => {});
  const { service, events, state } = createOpenService(native, { fs: { readdir: async () => [] }, ...failingHistory() });
  try {
    const initializing = service.initializeInitialLibrary('/Pictures/Main.library');
    await flush();
    native.complete('/Pictures/Main.library');

    await expect(initializing).resolves.toBeUndefined();
    expect(state.current).toBe('/Pictures/Main.library');
    expect(events).toEqual([
      ['library-switching', { path: '/Pictures/Main.library' }],
      ['library-switched', { path: '/Pictures/Main.library' }],
    ]);
    expect(warning).toHaveBeenCalledWith('[library] library bookkeeping failed', {
      libraryPath: '/Pictures/Main.library',
      message: 'config is read-only',
    });
  } finally {
    warning.mockRestore();
  }
});

test('a cloud join whose bookkeeping fails still publishes the joined library', async () => {
  const native = createNativeOpenStub();
  const join = cloudJoinOverrides();
  const warning = vi.spyOn(console, 'warn').mockImplementation(() => {});
  const { service, events, state } = createOpenService(native, { ...join.overrides, ...failingHistory() });
  state.current = '/Pictures/Main.library';
  try {
    const joining = service.joinCloudLibrary(join.request);
    await flush();
    native.complete('/Cloud/Shared.library');

    await expect(joining).resolves.toEqual({ joined: '/Cloud/Shared.library' });
    expect(state.current).toBe('/Cloud/Shared.library');
    expect(join.removed).toEqual([]);
    // The joined library is what is open: no failure, no return to the
    // previous library.
    expect(events).toEqual([
      ['library-switching', { path: '/Cloud/Shared.library' }],
      ['library-switched', { path: '/Cloud/Shared.library' }],
    ]);
    await expect(service.getLibraryConfig()).resolves.toMatchObject({
      currentPath: '/Cloud/Shared.library',
      openingPath: null,
    });
    expect(warning).toHaveBeenCalledWith('[library] library bookkeeping failed', {
      libraryPath: '/Cloud/Shared.library',
      message: 'config is read-only',
    });
  } finally {
    warning.mockRestore();
  }
});

test('a created library is listed even when its switch is superseded', async () => {
  const native = createNativeOpenStub();
  const { service, events, state } = createOpenService(native, { fs: { mkdir: async () => {} } });

  const creating = service.createLibrary({ name: 'New', savePath: '/Pictures' });
  await flush();
  expect(native.opening()).toEqual(['/Pictures/New.library']);
  // Listed as soon as it exists on disk.
  expect(state.histories).toEqual(['/Pictures/New.library']);
  const switching = service.switchLibrary('/Pictures/B.library');
  await flush();

  await expect(creating).resolves.toBe('/Pictures/New.library');
  native.complete('/Pictures/B.library');
  await switching;
  expect(state.current).toBe('/Pictures/B.library');
  expect(state.histories).toEqual(['/Pictures/New.library', '/Pictures/B.library']);
  expect(events.filter(([name]) => name === 'library-switched')).toEqual([
    ['library-switched', { path: '/Pictures/B.library' }],
  ]);
});

test('a created library still opens when listing it early fails', async () => {
  const native = createNativeOpenStub();
  const warning = vi.spyOn(console, 'warn').mockImplementation(() => {});
  let historyWrites = 0;
  const { service, state } = createOpenService(native, {
    fs: { mkdir: async () => {} },
    addLibraryToHistory: async (value) => {
      if (historyWrites++ === 0) throw new Error('config is read-only');
      state.histories.push(value);
    },
  });
  try {
    const creating = service.createLibrary({ name: 'New', savePath: '/Pictures' });
    await flush();
    native.complete('/Pictures/New.library');

    await expect(creating).resolves.toBe('/Pictures/New.library');
    expect(state.current).toBe('/Pictures/New.library');
    expect(state.histories).toEqual(['/Pictures/New.library']);
    expect(warning).toHaveBeenCalledWith('[library] library bookkeeping failed', {
      libraryPath: '/Pictures/New.library',
      message: 'config is read-only',
    });
  } finally {
    warning.mockRestore();
  }
});

test('a failure report whose close overlaps a switch still reports the failed library', async () => {
  const native = createNativeOpenStub();
  const pendingCloses = [];
  const { service, events, state } = createOpenService(native, {
    closeLibrary: async () => {
      await native.closeLibrary();
      await new Promise((resolve) => pendingCloses.push(resolve));
    },
  });
  state.current = '/Pictures/A.library';

  const failing = service.failActiveLibrary('database is unreadable');
  await flush();
  const switching = service.switchLibrary('/Pictures/B.library');
  await flush();
  expect(pendingCloses).toHaveLength(2);
  // The failure's close settles while the switch is in flight: the report
  // describes a library that is no longer current and reaches no window.
  pendingCloses.shift()();

  await expect(failing).resolves.toBeNull();
  expect(events.filter(([name]) => name === 'library-open-failed')).toEqual([]);

  pendingCloses.shift()();
  await flush();
  native.complete('/Pictures/B.library');
  await switching;
  expect(state.current).toBe('/Pictures/B.library');
  expect(events).toEqual([
    ['library-switching', { path: '/Pictures/B.library' }],
    ['library-switched', { path: '/Pictures/B.library' }],
  ]);
  await expect(service.getLibraryConfig()).resolves.toMatchObject({ libraryFailure: null });
});

test('a failure report whose close settles after a switch published keeps that library current', async () => {
  const native = createNativeOpenStub();
  const pendingCloses = [];
  const { service, events, state } = createOpenService(native, {
    closeLibrary: async () => {
      await native.closeLibrary();
      await new Promise((resolve) => pendingCloses.push(resolve));
    },
  });
  state.current = '/Pictures/A.library';

  const failing = service.failActiveLibrary('database is unreadable');
  await flush();
  const switching = service.switchLibrary('/Pictures/B.library');
  await flush();
  expect(pendingCloses).toHaveLength(2);
  // The switch's close settles first and B publishes.
  pendingCloses[1]();
  await flush();
  native.complete('/Pictures/B.library');
  await switching;
  expect(state.current).toBe('/Pictures/B.library');
  // Then the failure's close settles: B stays current and open.
  pendingCloses[0]();
  await expect(failing).resolves.toBeNull();
  expect(state.current).toBe('/Pictures/B.library');
  expect(events.filter(([name]) => name === 'library-open-failed')).toEqual([]);
});

test('a rename superseded while its history update fails still reports the renamed library', async () => {
  const native = createNativeOpenStub();
  const rename = renameOverrides();
  let failUpdate = () => {};
  const { service, state } = createOpenService(native, {
    ...rename.overrides,
    updateLibraryPath: () => new Promise((_resolve, reject) => {
      failUpdate = () => reject(new Error('config is read-only'));
    }),
  });
  state.current = '/Pictures/A.library';

  const renaming = service.renameLibrary('/Pictures/A.library', 'B');
  await flush();
  expect(rename.renames).toEqual([['/Pictures/A.library', '/Pictures/B.library']]);
  const switching = service.switchLibrary('/Pictures/C.library');
  await flush();
  failUpdate();

  // Renamed on disk: the library exists under its new name even though the
  // history could not record it, so the caller learns the new path.
  await expect(renaming).resolves.toEqual({ newPath: '/Pictures/B.library' });
  native.complete('/Pictures/C.library');
  await switching;
  expect(state.current).toBe('/Pictures/C.library');
});

test('a relocation superseded while its history update fails still reports the move', async () => {
  const native = createNativeOpenStub();
  const renames = [];
  let failUpdate = () => {};
  const { service, state } = createOpenService(native, {
    fs: {
      realpath: async (value) => value,
      access: async (target) => {
        if (target !== '/Pictures/A.library') throw Object.assign(new Error('missing'), { code: 'ENOENT' });
      },
      rename: async (...args) => { renames.push(args); },
    },
    dialog: { showOpenDialog: async () => ({ canceled: false, filePaths: ['/Archive'] }) },
    updateLibraryPath: () => new Promise((_resolve, reject) => {
      failUpdate = () => reject(new Error('config is read-only'));
    }),
  });
  state.current = '/Pictures/A.library';

  const relocating = service.relocateLibrary('/Pictures/A.library');
  await flush();
  expect(renames).toEqual([['/Pictures/A.library', '/Archive/A.library']]);
  const switching = service.switchLibrary('/Pictures/C.library');
  await flush();
  failUpdate();

  await expect(relocating).resolves.toEqual({ action: 'relocated', newPath: '/Archive/A.library' });
  native.complete('/Pictures/C.library');
  await switching;
  expect(state.current).toBe('/Pictures/C.library');
});

test.each([
  ['/Archive/Renamed.library', '/Archive/Renamed.library'],
  ['/Archive', '/Archive/A.library'],
])('Locate accepts a library package or its parent: %s', async (selected, expected) => {
  const updated = vi.fn();
  const { service } = createOpenService(createNativeOpenStub(), {
    fs: { access: async (target) => {
      if (target !== expected && target !== `${expected}/library.sqlite`) {
        throw Object.assign(new Error('missing'), { code: 'ENOENT' });
      }
    } },
    dialog: {
      showOpenDialog: async () => ({ canceled: false, filePaths: [selected] }),
      showMessageBox: async () => ({ response: 0 }),
    },
    updateLibraryPath: updated,
  });
  await expect(service.relocateLibrary('/Pictures/A.library')).resolves.toEqual({ action: 'relocated', newPath: expected });
  expect(updated).toHaveBeenCalledWith('/Pictures/A.library', expected);
  updated.mockClear();
  await expect(service.handleMissingLibrary('/Pictures/A.library')).resolves.toEqual({ action: 'relocated', newPath: expected });
  expect(updated).toHaveBeenCalledWith('/Pictures/A.library', expected);
});
