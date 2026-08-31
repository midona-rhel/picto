import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);

export async function isFailedCloudJoinDirectory(fs, targetRoot) {
  const entries = await fs.readdir(targetRoot, { withFileTypes: true }).catch(() => null);
  if (!entries || entries.length === 0) return false;
  return entries.every((entry) =>
    (entry.isDirectory() && entry.name === 'blobs')
    || (entry.isFile() && /^failed-cloud-join-[\w-]+\.sqlite$/.test(entry.name)),
  );
}

export function createLibraryHostService({
  fs,
  path,
  dialog,
  openLibrary,
  openTutorialLibrary,
  closeLibrary,
  invokeSerialized,
  addLibraryToHistory,
  removeLibraryFromHistory,
  togglePinned,
  getCachedConfig,
  saveGlobalConfig,
  updateLibraryPath,
  getCurrentLibraryRoot,
  setCurrentLibraryRoot,
  createMainWindow,
  sendToAllWindows,
  buildAppMenu,
  tutorialRoot,
  tutorialFixtureRoot,
  platform = process.platform,
  resourcesPath = process.resourcesPath ?? process.cwd(),
  isDefaultApp = process.defaultApp === true || !process.resourcesPath,
  runFileAttributeCommand = execFileAsync,
  setFileIcon = null,
}) {
  let openingLibraryPath = null;
  let tutorialSession = null;
  let libraryFailure = null;
  const coverExtensions = ['jpg', 'png'];

  function materializedCoverPath(libraryPath, extension) {
    return path.join(libraryPath, `.picto-library-cover.${extension}`);
  }

  function materializedCoverMetadataPath(libraryPath) {
    return path.join(libraryPath, '.picto-library-cover.json');
  }

  async function readMaterializedCoverMetadata(libraryPath) {
    try {
      const value = JSON.parse(await fs.readFile(materializedCoverMetadataPath(libraryPath), 'utf8'));
      if (typeof value.imageHash !== 'string' || !/^[a-fA-F0-9]{64}$/.test(value.imageHash)) return null;
      return {
        imageHash: value.imageHash,
        imageFocusX: Number.isFinite(value.imageFocusX) ? value.imageFocusX : 500,
        imageFocusY: Number.isFinite(value.imageFocusY) ? value.imageFocusY : 500,
        imageZoomPercent: Number.isFinite(value.imageZoomPercent) ? value.imageZoomPercent : 100,
      };
    } catch {
      return null;
    }
  }

  async function writeMaterializedCoverMetadata(libraryPath, meta) {
    const destination = materializedCoverMetadataPath(libraryPath);
    const temporary = `${destination}.${process.pid}.${Date.now()}.tmp`;
    await fs.writeFile(temporary, JSON.stringify({
      imageHash: meta.imageHash,
      imageFocusX: meta.imageFocusX ?? 500,
      imageFocusY: meta.imageFocusY ?? 500,
      imageZoomPercent: meta.imageZoomPercent ?? 100,
    }, null, 2), 'utf8');
    await fs.rename(temporary, destination);
  }

  async function clearMaterializedLibraryCover(libraryPath, exceptExtension = null) {
    await Promise.all(coverExtensions
      .filter((extension) => extension !== exceptExtension)
      .map((extension) => fs.rm(materializedCoverPath(libraryPath, extension), { force: true }).catch(() => {})));
  }

  async function materializeLibraryCover(libraryPath, imageHash) {
    if (typeof imageHash !== 'string' || !/^[a-fA-F0-9]{64}$/.test(imageHash)) return false;
    const directory = path.join(libraryPath, 'blobs', 't', imageHash.slice(0, 2), imageHash.slice(2, 4));
    for (const extension of coverExtensions) {
      const source = path.join(directory, `${imageHash}.${extension}`);
      if (!await fs.access(source).then(() => true, () => false)) continue;
      const destination = materializedCoverPath(libraryPath, extension);
      const temporary = `${destination}.${process.pid}.${Date.now()}.tmp`;
      await fs.copyFile(source, temporary);
      await fs.rename(temporary, destination);
      await clearMaterializedLibraryCover(libraryPath, extension);
      return true;
    }
    return false;
  }

  async function ensureMaterializedLibraryCover(libraryPath, imageHash) {
    const exists = await hasMaterializedLibraryCover(libraryPath);
    if (!exists) await materializeLibraryCover(libraryPath, imageHash).catch(() => false);
  }

  async function hasMaterializedLibraryCover(libraryPath) {
    return Promise.any(coverExtensions.map((extension) =>
      fs.access(materializedCoverPath(libraryPath, extension)).then(() => true),
    )).catch(() => false);
  }

  async function cleanupStaleTutorialLibraries() {
    if (!tutorialRoot) return;
    for (const entry of await fs.readdir(tutorialRoot, { withFileTypes: true }).catch(() => [])) {
      if (entry.isDirectory() && entry.name.startsWith('picto-guided-tour-')) {
        await fs.rm(path.join(tutorialRoot, entry.name), { recursive: true, force: true });
      }
    }
  }

  async function isValidLibrary(libraryPath) {
    try {
      await fs.access(path.join(libraryPath, 'library.sqlite'));
      return true;
    } catch {
      return false;
    }
  }

  function libraryDisplayName(libraryPath) {
    const base = path.basename(libraryPath);
    return base.endsWith('.library') ? base.slice(0, -8) : base;
  }

  async function applyPlatformLibraryIcon(libraryPath) {
    if (platform === 'darwin') {
      if (typeof setFileIcon !== 'function') return;
      const sourceIcon = isDefaultApp
        ? path.join(process.cwd(), 'build', 'library.icns')
        : path.join(resourcesPath, 'library-icons', 'library.icns');
      try {
        if (!await setFileIcon(sourceIcon, libraryPath)) {
          throw new Error('Finder rejected the custom library icon');
        }
      } catch (error) {
        console.warn('[library] unable to apply macOS package icon', {
          libraryPath,
          message: error?.message ?? String(error),
        });
      }
      return;
    }
    if (platform !== 'win32') return;
    const sourceIcon = isDefaultApp
      ? path.join(process.cwd(), 'build', 'library.ico')
      : path.join(resourcesPath, 'library-icons', 'library.ico');
    const iconName = '.picto-library.ico';
    const iconPath = path.join(libraryPath, iconName);
    const desktopIniPath = path.join(libraryPath, 'desktop.ini');
    try {
      await fs.copyFile(sourceIcon, iconPath);
      await fs.writeFile(desktopIniPath, `[.ShellClassInfo]\r\nIconResource=${iconName},0\r\n`, 'utf8');
      await runFileAttributeCommand('attrib', ['+h', '+s', iconPath]);
      await runFileAttributeCommand('attrib', ['+h', '+s', desktopIniPath]);
      await runFileAttributeCommand('attrib', ['+r', libraryPath]);
    } catch (error) {
      console.warn('[library] unable to apply Windows folder icon', {
        libraryPath,
        message: error?.message ?? String(error),
      });
    }
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
    await applyPlatformLibraryIcon(libraryPath);
    setCurrentLibraryRoot(libraryPath);
    await openLibrary(libraryPath);

    await addLibraryToHistory(libraryPath);
    buildAppMenu();
    createMainWindow();
  }

  async function switchLibrary(newPath) {
    if (tutorialSession) throw new Error('Exit the guided tour before switching libraries');
    openingLibraryPath = newPath;
    libraryFailure = null;
    sendToAllWindows('library-switching', { path: newPath });

    await applyPlatformLibraryIcon(newPath);

    await closeLibrary();
    try {
      await openLibrary(newPath);
    } catch (error) {
      openingLibraryPath = null;
      sendToAllWindows('library-open-failed', { path: newPath, message: error?.message ?? String(error) });
      throw error;
    }

    setCurrentLibraryRoot(newPath);
    openingLibraryPath = null;
    await addLibraryToHistory(newPath);
    buildAppMenu();

    sendToAllWindows('library-switched', { path: newPath });
  }

  async function startTutorialLibrary() {
    if (tutorialSession) return { path: tutorialSession.path };
    const previousPath = getCurrentLibraryRoot();
    if (!previousPath) throw new Error('Open a library before starting the guided tour');
    await cleanupStaleTutorialLibraries();
    const sessionPath = await fs.mkdtemp(path.join(tutorialRoot, 'picto-guided-tour-'));
    const libraryPath = path.join(sessionPath, 'Guided Tour.library');
    await fs.mkdir(path.join(libraryPath, 'blobs'), { recursive: true });
    tutorialSession = { path: libraryPath, sessionPath, previousPath };
    openingLibraryPath = libraryPath;
    sendToAllWindows('library-switching', { path: libraryPath, tutorial: true });
    try {
      await closeLibrary();
      await openTutorialLibrary(libraryPath, tutorialFixtureRoot);
      setCurrentLibraryRoot(libraryPath);
      await seedTutorialLibrary();
      openingLibraryPath = null;
      buildAppMenu();
      sendToAllWindows('library-switched', { path: libraryPath, tutorial: true });
      return { path: libraryPath };
    } catch (error) {
      openingLibraryPath = null;
      tutorialSession = null;
      await fs.rm(sessionPath, { recursive: true, force: true });
      await openLibrary(previousPath);
      setCurrentLibraryRoot(previousPath);
      sendToAllWindows('library-switched', { path: previousPath, restored: true });
      throw error;
    }
  }

  async function resetTutorialLibrary() {
    if (!tutorialSession) throw new Error('The guided tour is not running');
    const previousPath = tutorialSession.previousPath;
    const previousSessionPath = tutorialSession.sessionPath;
    const sessionPath = await fs.mkdtemp(path.join(tutorialRoot, 'picto-guided-tour-'));
    const libraryPath = path.join(sessionPath, 'Guided Tour.library');
    await fs.mkdir(path.join(libraryPath, 'blobs'), { recursive: true });
    openingLibraryPath = libraryPath;
    sendToAllWindows('library-switching', { path: libraryPath, tutorial: true });
    await closeLibrary();
    try {
      await fs.rm(previousSessionPath, { recursive: true, force: true });
      tutorialSession = { path: libraryPath, sessionPath, previousPath };
      await openTutorialLibrary(libraryPath, tutorialFixtureRoot);
      setCurrentLibraryRoot(libraryPath);
      await seedTutorialLibrary();
      openingLibraryPath = null;
      buildAppMenu();
      sendToAllWindows('library-switched', { path: libraryPath, tutorial: true });
      return { path: libraryPath };
    } catch (error) {
      openingLibraryPath = null;
      tutorialSession = null;
      await fs.rm(sessionPath, { recursive: true, force: true });
      await openLibrary(previousPath);
      setCurrentLibraryRoot(previousPath);
      sendToAllWindows('library-switched', { path: previousPath, restored: true });
      throw error;
    }
  }

  async function seedTutorialLibrary() {
    const command = async (name, args) => JSON.parse(await invokeSerialized(name, args));
    const referenceFolder = await command('folders.create', {
      name: 'Renaissance reference', parent_id: null, folder_key: 'tutorial-reference',
    });
    const studiesFolder = await command('folders.create', {
      name: 'Portrait studies', parent_id: referenceFolder.folder_id, folder_key: 'tutorial-studies',
    });
    await command('imports.enqueue', {
      paths: [
        path.join(tutorialFixtureRoot, 'seed-mona-detail.jpg'),
        path.join(tutorialFixtureRoot, 'seed-lady-detail.jpg'),
        path.join(tutorialFixtureRoot, 'duplicate-a.jpg'),
        path.join(tutorialFixtureRoot, 'duplicate-b.jpg'),
      ],
      tags: ['creator:leonardo da vinci', 'series:renaissance portraits'],
      source_urls: ['https://commons.wikimedia.org/'],
      lifecycle: 'active',
      parent_folder_id: studiesFolder.folder_id,
      preserve_structure: false,
      delete_after_ingest: false,
      group_files: false,
    });
    await command('imports.enqueue', {
      paths: [
        path.join(tutorialFixtureRoot, 'collection-a.jpg'),
        path.join(tutorialFixtureRoot, 'collection-b.jpg'),
      ],
      tags: ['creator:leonardo da vinci', 'meta:tutorial collection'],
      source_urls: ['https://commons.wikimedia.org/'],
      lifecycle: 'active',
      parent_folder_id: referenceFolder.folder_id,
      preserve_structure: false,
      delete_after_ingest: false,
      group_files: true,
    });
    await command('imports.enqueue', {
      paths: [
        path.join(tutorialFixtureRoot, 'inbox-study.jpg'),
        path.join(tutorialFixtureRoot, 'inbox-study-2.jpg'),
      ],
      tags: ['creator:leonardo da vinci'],
      source_urls: ['https://commons.wikimedia.org/'],
      lifecycle: 'inbox',
      parent_folder_id: null,
      preserve_structure: false,
      delete_after_ingest: false,
      group_files: false,
    });
    const smartGroup = await command('smart_folders.create', {
      name: 'Renaissance searches',
      parent_id: null,
      predicate: { groups: [] },
      icon: null,
      color: '#9b7b45',
      notes: 'A pass-through group for related saved searches',
      sort_field: null,
      sort_order: null,
    });
    await command('smart_folders.create', {
      name: 'Leonardo works',
      parent_id: smartGroup.smart_folder_id,
      predicate: {
        groups: [{
          match_mode: 'all',
          negate: false,
          rules: [{ field: 'tags', op: 'includes', value: 'creator:leonardo da vinci', value2: null, values: null }],
        }],
      },
      icon: null,
      color: '#6f8f72',
      notes: 'Media tagged with Leonardo da Vinci',
      sort_field: null,
      sort_order: null,
    });
    await command('settings.patch', {
      value: {
        sidebarQuickAccess: [
          `folder:${referenceFolder.folder_id}`,
          `smart:${smartGroup.smart_folder_id}`,
        ],
      },
    });
    const subscription = await command('subscriptions.create', {
      name: 'Leonardo da Vinci Archive',
      schedule: 'manual',
      initial_post_limit: 1,
      periodic_post_limit: 1,
      queries: [{
        site_id: 'twitter',
        query_text: 'LeonardoDaVinci',
        display_name: 'Leonardo da Vinci Archive',
        notes: 'Offline guided-tour source',
        group_posts: true,
      }],
    });
    await command('subscriptions.destination', {
      subscription_id: subscription.subscription_id,
      destination: {
        target_folder_ids: [referenceFolder.folder_id],
        automatic_tags: ['creator:leonardo da vinci', 'meta:tutorial subscription'],
      },
    });
  }

  async function finishTutorialLibrary() {
    if (!tutorialSession) return { restored: false };
    const session = tutorialSession;
    tutorialSession = null;
    openingLibraryPath = session.previousPath;
    sendToAllWindows('library-switching', { path: session.previousPath, tutorial: false });
    await closeLibrary();
    try {
      await openLibrary(session.previousPath);
      setCurrentLibraryRoot(session.previousPath);
      openingLibraryPath = null;
      buildAppMenu();
      sendToAllWindows('library-switched', { path: session.previousPath, restored: true });
    } finally {
      await fs.rm(session.sessionPath, { recursive: true, force: true });
    }
    return { restored: true, path: session.previousPath };
  }

  function getTutorialSession() {
    return tutorialSession ? { active: true, path: tutorialSession.path } : { active: false };
  }

  async function createLibrary({ name, savePath }) {
    const libraryPath = path.join(savePath, `${name}.library`);
    await fs.mkdir(libraryPath, { recursive: true });
    await fs.mkdir(path.join(libraryPath, 'blobs'), { recursive: true });
    await switchLibrary(libraryPath);
    return libraryPath;
  }

  async function joinCloudLibrary({ provider, accountLabel, rootPath, libraryId, name }) {
    const cleanName = String(name ?? '').trim();
    if (!cleanName) throw new Error('Library name cannot be empty');
    if (/[/\\]/.test(cleanName)) throw new Error('Library name cannot contain slashes');
    if (!['google_drive', 'dropbox'].includes(provider)) {
      throw new Error('Unsupported cloud folder provider');
    }
    const picked = await dialog.showOpenDialog({
      title: `Choose where to store "${cleanName}"`,
      properties: ['openDirectory', 'createDirectory'],
      message: 'Picto will restore the verified database here and recover media in the background.',
    });
    if (picked.canceled || picked.filePaths.length === 0) return null;
    const targetRoot = path.join(picked.filePaths[0], `${cleanName}.library`);
    if (await fs.access(targetRoot).then(() => true, () => false)) {
      if (await isFailedCloudJoinDirectory(fs, targetRoot)) {
        await fs.rm(targetRoot, { recursive: true, force: true });
      } else {
        throw new Error(`A library named "${cleanName}" already exists at that location`);
      }
    }

    const previousPath = getCurrentLibraryRoot();
    openingLibraryPath = targetRoot;
    libraryFailure = null;
    let joined = false;
    sendToAllWindows('library-switching', { path: targetRoot });
    try {
      const serialized = await invokeSerialized('cloud.library.join', {
        provider,
        account_label: accountLabel,
        root_path: rootPath,
        library_id: libraryId,
        target_root: targetRoot,
      });
      const result = JSON.parse(serialized);
      joined = true;
      await applyPlatformLibraryIcon(targetRoot);
      setCurrentLibraryRoot(targetRoot);
      openingLibraryPath = null;
      await addLibraryToHistory(targetRoot);
      await setLibraryMeta(targetRoot, { cloudLibraryId: libraryId });
      buildAppMenu();
      sendToAllWindows('library-switched', { path: targetRoot });
      return result;
    } catch (error) {
      openingLibraryPath = null;
      if (!joined) await fs.rm(targetRoot, { recursive: true, force: true }).catch(() => {});
      sendToAllWindows('library-open-failed', {
        path: targetRoot,
        message: error?.message ?? String(error),
      });
      if (previousPath) sendToAllWindows('library-switched', { path: previousPath });
      throw error;
    }
  }

  async function openLibraryDialog() {
    const result = await dialog.showOpenDialog({
      title: 'Open Library',
      // macOS exposes package directories such as .library as files.
      properties: ['openFile', 'openDirectory'],
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
  
      await closeLibrary();
      try {
        await fs.rename(libraryPath, newPath);
      } catch (err) {
        await openLibrary(libraryPath);
    
        sendToAllWindows('library-switched', { path: libraryPath });
        throw new Error(`Failed to rename library: ${err.message}`);
      }
      await updateLibraryPath(libraryPath, newPath);
      setCurrentLibraryRoot(newPath);
      await openLibrary(newPath);
  
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
    
        await closeLibrary();
        try {
          await fs.rename(oldPath, newPath);
        } catch (err) {
          await openLibrary(oldPath);
      
          sendToAllWindows('library-switched', { path: oldPath });
          throw new Error(`Failed to move library: ${err.message}`);
        }
        await updateLibraryPath(oldPath, newPath);
        setCurrentLibraryRoot(newPath);
        await openLibrary(newPath);
    
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
    const coverExistsMap = {};
    const libraryMeta = { ...(config.libraryMeta ?? {}) };
    let recoveredCoverMetadata = false;
    await Promise.all(
      (config.libraryHistory ?? []).map(async (libraryPath) => {
        try {
          await fs.access(libraryPath);
          existsMap[libraryPath] = true;
          coverExistsMap[libraryPath] = await hasMaterializedLibraryCover(libraryPath);
          if (coverExistsMap[libraryPath] && !libraryMeta[libraryPath]?.imageHash) {
            const recovered = await readMaterializedCoverMetadata(libraryPath);
            if (recovered) {
              libraryMeta[libraryPath] = { ...(libraryMeta[libraryPath] ?? {}), ...recovered };
              recoveredCoverMetadata = true;
            }
          }
          const imageHash = libraryMeta[libraryPath]?.imageHash;
          if (imageHash && !coverExistsMap[libraryPath]) {
            await ensureMaterializedLibraryCover(libraryPath, imageHash);
            coverExistsMap[libraryPath] = await hasMaterializedLibraryCover(libraryPath);
          }
        } catch {
          existsMap[libraryPath] = false;
          coverExistsMap[libraryPath] = false;
        }
      }),
    );
    if (recoveredCoverMetadata) {
      config.libraryMeta = libraryMeta;
      await saveGlobalConfig(config);
    }
    return {
      ...config,
      libraryMeta,
      currentPath: getCurrentLibraryRoot(),
      openingPath: openingLibraryPath,
      libraryFailure,
      existsMap,
      coverExistsMap,
    };
  }

  async function rememberCloudRoot(root) {
    const provider = String(root?.provider ?? '');
    const accountLabel = String(root?.account_label ?? '').trim();
    const rootPath = String(root?.path ?? '').trim();
    if (!['google_drive', 'dropbox'].includes(provider) || !accountLabel || !rootPath) {
      throw new Error('Invalid cloud folder location');
    }
    const config = getCachedConfig();
    config.cloudLocations = {
      ...(config.cloudLocations ?? {}),
      [provider]: { provider, account_label: accountLabel, path: rootPath },
    };
    await saveGlobalConfig(config);
    return config.cloudLocations;
  }

  async function failActiveLibrary(message) {
    const failureMessage = String(message || 'The library navigation could not be loaded.');
    const failedPath = getCurrentLibraryRoot();
    libraryFailure = { path: failedPath, message: failureMessage };
    openingLibraryPath = null;
    if (failedPath) {
      try {
        await closeLibrary();
      } catch (error) {
        console.error('[library] failed to close after an initial read failure', error);
      }
    }
    setCurrentLibraryRoot(null);
    buildAppMenu();
    sendToAllWindows('library-open-failed', libraryFailure);
    return libraryFailure;
  }

  async function setLibraryMeta(libraryPath, meta) {
    if ('imageHash' in meta) {
      const hasCanonicalHash = typeof meta.imageHash === 'string'
        && /^[a-fA-F0-9]{64}$/.test(meta.imageHash);
      if (hasCanonicalHash) {
        if (!await materializeLibraryCover(libraryPath, meta.imageHash)) {
          throw new Error('The selected library cover thumbnail is not available');
        }
      } else if (!meta.imageHash) {
        await clearMaterializedLibraryCover(libraryPath);
        await fs.rm(materializedCoverMetadataPath(libraryPath), { force: true }).catch(() => {});
      }
    }
    // Materializing a cover is asynchronous. Re-read the current config after
    // it finishes so an overlapping theme/window-state save cannot be replaced
    // by the stale object captured before the file operation.
    const config = getCachedConfig();
    if (!config.libraryMeta) config.libraryMeta = {};
    const existing = { ...(config.libraryMeta[libraryPath] ?? {}) };
    if ('icon' in meta) existing.icon = meta.icon;
    if ('color' in meta) existing.color = meta.color;
    if ('imageHash' in meta) existing.imageHash = meta.imageHash;
    if ('imageFocusX' in meta) existing.imageFocusX = meta.imageFocusX;
    if ('imageFocusY' in meta) existing.imageFocusY = meta.imageFocusY;
    if ('imageZoomPercent' in meta) existing.imageZoomPercent = meta.imageZoomPercent;
    if ('cloudLibraryId' in meta) existing.cloudLibraryId = meta.cloudLibraryId;
    if (typeof existing.imageHash === 'string' && /^[a-fA-F0-9]{64}$/.test(existing.imageHash)) {
      await writeMaterializedCoverMetadata(libraryPath, existing);
    }
    config.libraryMeta[libraryPath] = existing;
    await saveGlobalConfig(config);
    sendToAllWindows('library-meta-changed', { path: libraryPath });
  }

  async function initializeInitialLibrary(libraryPath, { remember = true } = {}) {
    await cleanupStaleTutorialLibraries();
    await applyPlatformLibraryIcon(libraryPath);
    openingLibraryPath = libraryPath;
    sendToAllWindows('library-switching', { path: libraryPath });
    try {
      await openLibrary(libraryPath);
    } catch (error) {
      openingLibraryPath = null;
      sendToAllWindows('library-open-failed', { path: libraryPath, message: error?.message ?? String(error) });
      throw error;
    }
    setCurrentLibraryRoot(libraryPath);
    openingLibraryPath = null;
    if (remember) await addLibraryToHistory(libraryPath);
    sendToAllWindows('library-switched', { path: libraryPath });
  }

  return {
    createLibrary,
    deleteLibrary,
    getLibraryConfig,
    handleMissingLibrary,
    initializeInitialLibrary,
    isValidLibrary,
    libraryDisplayName,
    joinCloudLibrary,
    openLibraryAndShow,
    openLibraryDialog,
    relocateLibrary,
    removeLibrary,
    renameLibrary,
    rememberCloudRoot,
    setLibraryMeta,
    startTutorialLibrary,
    resetTutorialLibrary,
    finishTutorialLibrary,
    failActiveLibrary,
    getTutorialSession,
    switchLibrary,
    toggleLibraryPin,
  };
}
