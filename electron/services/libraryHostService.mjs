export function createLibraryHostService({
  fs,
  path,
  dialog,
  initialize,
  openLibrary,
  closeLibrary,
  addLibraryToHistory,
  removeLibraryFromHistory,
  togglePinned,
  getCachedConfig,
  saveGlobalConfig,
  updateLibraryPath,
  getCurrentLibraryRoot,
  setCurrentLibraryRoot,
  startBlurhashBackfill,
  stopBlurhashBackfill,
  createMainWindow,
  sendToAllWindows,
  buildAppMenu,
}) {
  async function isValidLibrary(libraryPath) {
    try {
      await fs.access(path.join(libraryPath, 'db', 'library.sqlite'));
      return true;
    } catch {
      try {
        await fs.access(libraryPath);
        return true;
      } catch {
        return false;
      }
    }
  }

  function libraryDisplayName(libraryPath) {
    const base = path.basename(libraryPath);
    return base.endsWith('.library') ? base.slice(0, -8) : base;
  }

  async function handleMissingLibrary(libraryPath) {
    const name = libraryDisplayName(libraryPath);
    const result = await dialog.showMessageBox({
      type: 'warning',
      title: 'Library Not Found',
      message: `Library "${name}" could not be found.`,
      detail: `Path: ${libraryPath}`,
      buttons: ['Locate…', 'Remove from List', 'Cancel'],
      defaultId: 0,
      cancelId: 2,
    });

    if (result.response === 0) {
      const basename = path.basename(libraryPath);
      const picked = await dialog.showOpenDialog({
        title: `Locate "${name}" library`,
        properties: ['openDirectory', 'createDirectory'],
        message: `Select the folder containing ${basename}`,
      });
      if (!picked.canceled && picked.filePaths.length > 0) {
        const destDir = picked.filePaths[0];
        const newPath = path.join(destDir, basename);
        if (newPath === libraryPath) return { action: 'cancelled' };
        const exists = await fs.access(newPath).then(() => true, () => false);
        if (!exists) {
          await dialog.showMessageBox({
            type: 'error',
            title: 'Library Not Found',
            message: `"${basename}" was not found in the selected folder.`,
          });
          return { action: 'cancelled' };
        }
        await updateLibraryPath(libraryPath, newPath);
        return { action: 'relocated', newPath };
      }
      return { action: 'cancelled' };
    }

    if (result.response === 1) {
      await removeLibraryFromHistory(libraryPath);
      return { action: 'removed' };
    }

    return { action: 'cancelled' };
  }

  async function openLibraryAndShow(libraryPath) {
    setCurrentLibraryRoot(libraryPath);
    await openLibrary(libraryPath);
    startBlurhashBackfill();
    await addLibraryToHistory(libraryPath);
    buildAppMenu();
    createMainWindow();
  }

  async function switchLibrary(newPath) {
    sendToAllWindows('library-switching', { path: newPath });
    stopBlurhashBackfill();
    await closeLibrary();

    setCurrentLibraryRoot(newPath);
    await openLibrary(newPath);
    startBlurhashBackfill();
    await addLibraryToHistory(newPath);
    buildAppMenu();

    sendToAllWindows('library-switched', { path: newPath });
  }

  async function createLibrary({ name, savePath }) {
    const libraryPath = path.join(savePath, `${name}.library`);
    await fs.mkdir(path.join(libraryPath, 'db'), { recursive: true });
    await fs.mkdir(path.join(libraryPath, 'blobs'), { recursive: true });
    await fs.mkdir(path.join(libraryPath, 'plugins', 'sites'), { recursive: true });
    await switchLibrary(libraryPath);
    return libraryPath;
  }

  async function openLibraryDialog() {
    const result = await dialog.showOpenDialog({
      title: 'Open Library',
      properties: ['openDirectory'],
      message: 'Select a .library folder',
    });
    if (result.canceled || result.filePaths.length === 0) return null;
    const picked = result.filePaths[0];
    if (!path.basename(picked).endsWith('.library')) {
      await dialog.showMessageBox({
        type: 'error',
        title: 'Invalid Library',
        message: 'The selected folder is not a library.',
        detail: 'Please select a folder ending in .library',
      });
      return null;
    }
    await switchLibrary(picked);
    return picked;
  }

  async function removeLibrary(libraryPath) {
    await removeLibraryFromHistory(libraryPath);
    buildAppMenu();
  }

  async function deleteLibrary(libraryPath) {
    if (libraryPath === getCurrentLibraryRoot()) {
      throw new Error('Cannot delete the currently open library. Switch to a different library first.');
    }
    const name = libraryDisplayName(libraryPath);
    const result = await dialog.showMessageBox({
      type: 'warning',
      title: 'Delete Library',
      message: `Delete library "${name}"?`,
      detail: `This will permanently delete all files in:\n${libraryPath}\n\nThis cannot be undone.`,
      buttons: ['Delete', 'Cancel'],
      defaultId: 1,
      cancelId: 1,
      destructiveId: 0,
    });
    if (result.response !== 0) return { deleted: false };
    await removeLibraryFromHistory(libraryPath);
    await fs.rm(libraryPath, { recursive: true, force: true });
    buildAppMenu();
    return { deleted: true };
  }

  async function toggleLibraryPin(libraryPath) {
    await togglePinned(libraryPath);
    buildAppMenu();
  }

  async function renameLibrary(libraryPath, newName) {
    if (!newName || typeof newName !== 'string' || !newName.trim()) {
      throw new Error('Library name cannot be empty');
    }
    const cleanName = newName.trim();
    if (/[/\\]/.test(cleanName)) {
      throw new Error('Library name cannot contain slashes');
    }

    const parentDir = path.dirname(libraryPath);
    const newPath = path.join(parentDir, `${cleanName}.library`);

    try {
      await fs.access(newPath);
      throw new Error(`A library named "${cleanName}" already exists at that location`);
    } catch (err) {
      if (err.code !== 'ENOENT') throw err;
    }

    if (libraryPath === getCurrentLibraryRoot()) {
      sendToAllWindows('library-switching', { path: newPath });
      stopBlurhashBackfill();
      await closeLibrary();
      try {
        await fs.rename(libraryPath, newPath);
      } catch (err) {
        await openLibrary(libraryPath);
        startBlurhashBackfill();
        sendToAllWindows('library-switched', { path: libraryPath });
        throw new Error(`Failed to rename library: ${err.message}`);
      }
      await updateLibraryPath(libraryPath, newPath);
      setCurrentLibraryRoot(newPath);
      await openLibrary(newPath);
      startBlurhashBackfill();
      sendToAllWindows('library-switched', { path: newPath });
      buildAppMenu();
    } else {
      await fs.rename(libraryPath, newPath);
      await updateLibraryPath(libraryPath, newPath);
      buildAppMenu();
    }

    return { newPath };
  }

  async function relocateLibrary(oldPath) {
    const name = libraryDisplayName(oldPath);
    const basename = path.basename(oldPath);
    const oldExists = await fs.access(oldPath).then(() => true, () => false);

    const picked = await dialog.showOpenDialog({
      title: `Choose destination for "${name}"`,
      properties: ['openDirectory', 'createDirectory'],
      message: `${basename} will be placed in the selected folder`,
    });
    if (picked.canceled || picked.filePaths.length === 0) {
      return { action: 'cancelled' };
    }

    const destDir = picked.filePaths[0];
    const newPath = path.join(destDir, basename);
    if (newPath === oldPath) return { action: 'cancelled' };

    const destExists = await fs.access(newPath).then(() => true, () => false);
    if (destExists) {
      await dialog.showMessageBox({
        type: 'error',
        title: 'Already Exists',
        message: `"${basename}" already exists at that location.`,
      });
      return { action: 'cancelled' };
    }

    if (oldExists) {
      if (oldPath === getCurrentLibraryRoot()) {
        sendToAllWindows('library-switching', { path: newPath });
        stopBlurhashBackfill();
        await closeLibrary();
        try {
          await fs.rename(oldPath, newPath);
        } catch (err) {
          await openLibrary(oldPath);
          startBlurhashBackfill();
          sendToAllWindows('library-switched', { path: oldPath });
          throw new Error(`Failed to move library: ${err.message}`);
        }
        await updateLibraryPath(oldPath, newPath);
        setCurrentLibraryRoot(newPath);
        await openLibrary(newPath);
        startBlurhashBackfill();
        sendToAllWindows('library-switched', { path: newPath });
      } else {
        await fs.rename(oldPath, newPath);
        await updateLibraryPath(oldPath, newPath);
      }
    } else {
      await updateLibraryPath(oldPath, newPath);
    }

    buildAppMenu();
    return { action: 'relocated', newPath };
  }

  async function getLibraryConfig() {
    const config = getCachedConfig();
    const existsMap = {};
    await Promise.all(
      (config.libraryHistory ?? []).map(async (libraryPath) => {
        try {
          await fs.access(libraryPath);
          existsMap[libraryPath] = true;
        } catch {
          existsMap[libraryPath] = false;
        }
      }),
    );
    return {
      ...config,
      currentPath: getCurrentLibraryRoot(),
      existsMap,
    };
  }

  async function initializeInitialLibrary(libraryPath) {
    setCurrentLibraryRoot(libraryPath);
    await initialize(libraryPath);
    await addLibraryToHistory(libraryPath);
  }

  return {
    createLibrary,
    deleteLibrary,
    getLibraryConfig,
    handleMissingLibrary,
    initializeInitialLibrary,
    isValidLibrary,
    libraryDisplayName,
    openLibraryAndShow,
    openLibraryDialog,
    relocateLibrary,
    removeLibrary,
    renameLibrary,
    switchLibrary,
    toggleLibraryPin,
  };
}
