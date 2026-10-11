import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { LIBRARY_OPEN_CANCELLED, isLibraryOpenCancelled } from '../libraryOpenCancellation.mjs';

const execFileAsync = promisify(execFile);

export async function isFailedCloudJoinDirectory(fs, targetRoot) {
  const entries = await fs.readdir(targetRoot, { withFileTypes: true }).catch(() => null);
  if (!entries || entries.length === 0) return false;
  return entries.every((entry) =>
    (entry.isDirectory() && entry.name === 'blobs')
    || (entry.isFile() && /^failed-cloud-join-[\w-]+\.sqlite$/.test(entry.name)),
  );
}

export function isInsideLibraryPackage(pathApi, candidatePath) {
  let current = pathApi.resolve(candidatePath);
  while (true) {
    if (pathApi.basename(current).toLowerCase().endsWith('.library')) return true;
    const parent = pathApi.dirname(current);
    if (parent === current) return false;
    current = parent;
  }
}

async function verifyDirectoryCopy(fs, pathApi, source, destination) {
  let sourceCount = 0;
  // Stream one directory at a time; library-sized manifests can exhaust the main-process heap.
  for await (const entry of await fs.opendir(source)) {
    sourceCount += 1;
    const original = pathApi.join(source, entry.name);
    const copied = pathApi.join(destination, entry.name);
    const copiedStat = await fs.lstat(copied);
    let matches = false;
    if (entry.isDirectory()) {
      matches = copiedStat.isDirectory();
      if (matches) await verifyDirectoryCopy(fs, pathApi, original, copied);
    } else if (entry.isFile()) {
      matches = copiedStat.isFile()
        && (await fs.stat(original)).size === copiedStat.size
        && await filesEqual(fs, original, copied);
    } else if (entry.isSymbolicLink()) {
      matches = copiedStat.isSymbolicLink()
        && await fs.readlink(original) === await fs.readlink(copied);
    }
    if (!matches) throw new Error('Copied library verification failed');
  }
  let destinationCount = 0;
  for await (const _entry of await fs.opendir(destination)) destinationCount += 1;
  if (destinationCount !== sourceCount) {
    throw new Error('Copied library verification failed');
  }
}

async function filesEqual(fs, source, destination) {
  const sourceHandle = await fs.open(source, 'r');
  let destinationHandle;
  try {
    destinationHandle = await fs.open(destination, 'r');
    const sourceBuffer = Buffer.allocUnsafe(1024 * 1024);
    const destinationBuffer = Buffer.allocUnsafe(sourceBuffer.length);
    let position = 0;
    while (true) {
      const [sourceRead, destinationRead] = await Promise.all([
        sourceHandle.read(sourceBuffer, 0, sourceBuffer.length, position),
        destinationHandle.read(destinationBuffer, 0, destinationBuffer.length, position),
      ]);
      if (sourceRead.bytesRead !== destinationRead.bytesRead) return false;
      if (sourceRead.bytesRead === 0) return true;
      if (!sourceBuffer.subarray(0, sourceRead.bytesRead)
        .equals(destinationBuffer.subarray(0, destinationRead.bytesRead))) return false;
      position += sourceRead.bytesRead;
    }
  } finally {
    await Promise.all([
      sourceHandle.close(),
      destinationHandle?.close(),
    ]);
  }
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
  let tutorialSession = null;
  // The session a guided-tour finish is restoring from, kept until that
  // finish settles. The tour is over as soon as the finish starts (switching
  // is allowed again and supersedes the restore); a tour started meanwhile
  // returns to this session's previous library, not to the tour directory.
  let finishingSession = null;
  let libraryFailure = null;
  // Every open/close flow claims an attempt. Only the newest attempt owns the
  // UI state (opening path, current root, history, menu, events); an older
  // attempt whose steps settle later must stay silent. An attempt records
  // its outcome once it settles: 'published', or one of the failure outcomes
  // of `settleFailedOpen`.
  let currentAttempt = null;
  let shuttingDown = false;
  let relocationSequence = 0;
  const deletingLibraries = new Set();
  const coverExtensions = ['jpg', 'png'];

  function markShuttingDown() {
    shuttingDown = true;
  }

  function isShuttingDown() {
    return shuttingDown;
  }

  function beginAttempt(libraryPath) {
    currentAttempt = { path: libraryPath, outcome: null };
    return currentAttempt;
  }

  function ownsAttempt(attempt) {
    return currentAttempt === attempt;
  }

  /** The library the attempt in flight is opening; null once it settled. */
  function openingPath() {
    return currentAttempt !== null && currentAttempt.outcome === null ? currentAttempt.path : null;
  }

  /**
   * Refuses to go on once a newer attempt owns the UI. Every native close or
   * open a flow issues is preceded by this check in the same tick, so that a
   * superseded flow never closes the library the newer attempt is opening.
   */
  function ensureOwned(attempt) {
    if (!ownsAttempt(attempt)) throw new Error(LIBRARY_OPEN_CANCELLED);
  }

  /**
   * Closes the open library on behalf of `attempt`. Nothing is open until an
   * open publishes: a root that still names the closed library makes every
   * entry point refuse to open it again and delete refuse to remove it.
   */
  async function closeUnderAttempt(attempt) {
    ensureOwned(attempt);
    try {
      await closeLibrary();
    } finally {
      if (ownsAttempt(attempt)) setCurrentLibraryRoot(null);
    }
  }

  /**
   * Starts the native open of `attempt`. Every flow starts its open here:
   * once the host is shutting down, or once a newer attempt owns the UI, the
   * open is refused with the cancellation sentinel instead of being started,
   * so nothing opens after the shutdown close or behind a newer switch.
   * `settleFailedOpen` classifies the refusal as 'shutdown' or 'superseded'.
   */
  async function startNativeOpen(attempt, open) {
    if (deletingLibraries.size) await assertNotDeleting(attempt.path);
    if (shuttingDown || !ownsAttempt(attempt)) throw new Error(LIBRARY_OPEN_CANCELLED);
    return open();
  }

  async function assertNotDeleting(libraryPath) {
    const reserved = new Set(deletingLibraries);
    const resolved = await fs.realpath(libraryPath).catch(() => null);
    if (!resolved || reserved.has(resolved)) {
      throw new Error('Cannot open this library while deletion is in progress.');
    }
  }

  function reportOpenFailure(libraryPath, error, cancelled = false) {
    sendToAllWindows('library-open-failed', {
      path: libraryPath,
      message: error?.message ?? String(error),
      ...(cancelled ? { cancelled: true } : {}),
    });
  }

  /**
   * Classifies a rejection of `attempt`; a settled attempt keeps its first
   * outcome, except that losing ownership always reads as superseded. Sends
   * nothing itself (see `reportSettledFailure`). Returns:
   * - 'superseded': a newer attempt owns the UI; nothing may be touched or
   *   sent for this one.
   * - 'shutdown': the host is quitting; nothing is sent. The shutdown close
   *   lands underneath whatever is in flight, so the error it surfaces is not
   *   always the sentinel (a close right after the open published fails the
   *   theme read instead); the flag decides, not the message.
   * - 'cancelled': cancelled by an explicit close.
   * - 'failed': any other error.
   * - 'published': the attempt had already published; the error came after.
   */
  function settleFailedOpen(attempt, error) {
    if (attempt.outcome === 'published') return 'published';
    if (!ownsAttempt(attempt)) return 'superseded';
    if (attempt.outcome === null) {
      attempt.outcome = shuttingDown ? 'shutdown' : isLibraryOpenCancelled(error) ? 'cancelled' : 'failed';
    }
    return attempt.outcome;
  }

  /**
   * Sends `library-open-failed` once for an attempt that settled as failed
   * or cancelled (`cancelled: true` marks the latter). A flow that falls
   * back to another library instead does not call this; `runAttempt` does
   * for everything else.
   */
  function reportSettledFailure(attempt, error) {
    const outcome = settleFailedOpen(attempt, error);
    if ((outcome !== 'failed' && outcome !== 'cancelled') || attempt.reported) return;
    attempt.reported = true;
    reportOpenFailure(attempt.path, error, outcome === 'cancelled');
  }

  /** Makes the library `attempt` opened current and tells every window. */
  function publishOpen(attempt, payload = {}) {
    ensureOwned(attempt);
    attempt.outcome = 'published';
    setCurrentLibraryRoot(attempt.path);
    sendToAllWindows('library-switched', { path: attempt.path, ...payload });
  }

  /**
   * Runs `flow` under a fresh attempt for `libraryPath`. Whatever the flow
   * rejects with is settled here, so that no rejection between claiming the
   * attempt and its native open (a close failing its checkpoint, a rename or
   * history write failing) can leave the opening path set forever: the
   * failure is reported to the windows unless the flow already fell back,
   * stays silent while shutting down, and is rethrown. A superseded flow
   * resolves with `onSuperseded(error)` when given, else rejects with the
   * cancellation sentinel whatever error the takeover surfaced as.
   */
  async function runAttempt(libraryPath, flow, { onSuperseded } = {}) {
    if (deletingLibraries.size) await assertNotDeleting(libraryPath);
    const attempt = beginAttempt(libraryPath);
    try {
      return await flow(attempt);
    } catch (error) {
      if (settleFailedOpen(attempt, error) !== 'superseded') {
        reportSettledFailure(attempt, error);
        throw error;
      }
      if (onSuperseded) return onSuperseded(error);
      throw new Error(LIBRARY_OPEN_CANCELLED);
    }
  }

  /**
   * Re-opens `libraryPath` under `attempt` after that attempt closed the
   * library and then failed before its own open could publish (a guided-tour
   * start/reset that failed, a rename/move whose disk operation failed).
   * Returns 'published' once `library-switched` went out, or 'superseded'
   * when a newer attempt owns the UI. A reopen that fails or is cancelled
   * leaves nothing open (root null) and rethrows.
   */
  async function reopenUnderAttempt(attempt, libraryPath, payload = {}) {
    if (!ownsAttempt(attempt)) return 'superseded';
    // The earlier failure was classified and not reported; the attempt now
    // opens the fallback, whose own failure is what the windows learn of.
    attempt.path = libraryPath;
    attempt.outcome = null;
    try {
      await startNativeOpen(attempt, () => openLibrary(libraryPath));
      publishOpen(attempt, payload);
      return 'published';
    } catch (error) {
      if (settleFailedOpen(attempt, error) === 'superseded') return 'superseded';
      throw error;
    }
  }

  /**
   * Runs bookkeeping (history, library meta, menu) for a library that is
   * open and published, or valid on disk. What the library is does not
   * depend on this: a failure is logged, never surfaced as an open failure
   * or as a failure of the flow.
   */
  async function recordLibrary(libraryPath, record) {
    try {
      await record();
    } catch (error) {
      console.warn('[library] library bookkeeping failed', {
        libraryPath,
        message: error?.message ?? String(error),
      });
    }
  }

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
      if (!entry.isDirectory() || !entry.name.startsWith('picto-guided-tour-')) continue;
      const sessionPath = path.join(tutorialRoot, entry.name);
      // A finish still closing its tour library owns that directory; the
      // next cleanup removes it.
      if (sessionPath === finishingSession?.sessionPath) continue;
      await fs.rm(sessionPath, { recursive: true, force: true });
    }
  }

  async function removeTutorialSessionDirectory(session) {
    try {
      await fs.rm(session.sessionPath, { recursive: true, force: true });
    } catch (error) {
      console.warn('[library] unable to remove the guided-tour session directory', {
        sessionPath: session.sessionPath,
        message: error?.message ?? String(error),
      });
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

  async function ownedLibraryPath(libraryPath) {
    try {
      const entry = await fs.lstat(libraryPath);
      if (!entry.isDirectory() || entry.isSymbolicLink()) throw new Error('not a real directory');
      const resolved = await fs.realpath(libraryPath);
      if (!path.basename(resolved).toLowerCase().endsWith('.library')) {
        throw new Error('not a library package');
      }
      const database = await fs.lstat(path.join(resolved, 'library.sqlite'));
      if (!database.isFile()) throw new Error('missing library database');
      return resolved;
    } catch {
      throw new Error('Refusing to delete a path that is no longer a Picto library.');
    }
  }

  async function moveLibraryDirectory(oldPath, newPath) {
    try {
      await fs.rename(oldPath, newPath);
      return;
    } catch (error) {
      if (error?.code !== 'EXDEV') throw error;
    }

    const stagingPath = path.join(
      path.dirname(newPath),
      `.${path.basename(newPath)}.picto-moving-${process.pid}-${++relocationSequence}`,
    );
    let published = false;
    try {
      await fs.cp(oldPath, stagingPath, {
        recursive: true,
        force: false,
        errorOnExist: true,
        preserveTimestamps: true,
        verbatimSymlinks: true,
      });
      await verifyDirectoryCopy(fs, path, oldPath, stagingPath);
      const destinationExists = await fs.access(newPath).then(() => true, (error) => {
        if (error?.code === 'ENOENT') return false;
        throw error;
      });
      if (destinationExists) throw new Error('The move destination appeared while copying the library');
      await fs.rename(stagingPath, newPath);
      published = true;
    } finally {
      if (!published) await fs.rm(stagingPath, { recursive: true, force: true }).catch(() => {});
    }

    try {
      await fs.rm(oldPath, { recursive: true, force: false });
    } catch (error) {
      console.warn('[library] unable to remove the original after a verified cross-volume copy', {
        oldPath,
        newPath,
        message: error?.message ?? String(error),
      });
    }
  }

  function libraryDisplayName(libraryPath) {
    const base = path.basename(libraryPath);
    return base.endsWith('.library') ? base.slice(0, -8) : base;
  }

  async function writeWindowsShellFile(filePath, contents) {
    const previous = await fs.readFile(filePath).catch((error) => {
      if (error.code === 'ENOENT') return null;
      throw error;
    });
    const changed = !previous?.equals(contents);
    let exists = previous !== null;
    try {
      // Windows refuses to overwrite hidden shell metadata. Only clear our
      // shell attributes when the bytes changed, and restore them on failure.
      if (changed && exists) await runFileAttributeCommand('attrib', ['-h', '-s', filePath]);
      if (changed) {
        await fs.writeFile(filePath, contents);
        exists = true;
      }
    } finally {
      if (exists) await runFileAttributeCommand('attrib', ['+h', '+s', filePath]);
    }
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
      await writeWindowsShellFile(iconPath, await fs.readFile(sourceIcon));
      await writeWindowsShellFile(desktopIniPath, Buffer.from(`[.ShellClassInfo]\r\nIconResource=${iconName},0\r\n`, 'utf8'));
      await runFileAttributeCommand('attrib', ['+r', libraryPath]);
    } catch (error) {
      console.warn('[library] unable to apply Windows folder icon', {
        libraryPath,
        message: error?.message ?? String(error),
      });
    }
  }

  async function locatedLibraryPath(selectedPath, basename) {
    for (const candidate of [selectedPath, path.join(selectedPath, basename)]) {
      if (await fs.access(path.join(candidate, 'library.sqlite')).then(() => true, () => false)) {
        return candidate;
      }
    }
    return null;
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
        const newPath = await locatedLibraryPath(picked.filePaths[0], basename);
        if (newPath === libraryPath) return { action: 'cancelled' };
        if (!newPath) {
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

  async function switchLibrary(newPath) {
    if (tutorialSession) throw new Error('Exit the guided tour before switching libraries');
    // The requested library is already opening (a double-click, a repeated
    // menu click): restarting the open would only throw away its progress.
    if (openingPath() === newPath) return;
    libraryFailure = null;
    return runAttempt(newPath, async (attempt) => {
      sendToAllWindows('library-switching', { path: newPath });
      await applyPlatformLibraryIcon(newPath);
      await closeUnderAttempt(attempt);
      await startNativeOpen(attempt, () => openLibrary(newPath));
      // Published first: the library is open once its native open completed,
      // and the bookkeeping that follows cannot make it not open.
      publishOpen(attempt);
      await recordLibrary(newPath, () => addLibraryToHistory(newPath));
      buildAppMenu();
    }, { onSuperseded: () => undefined });
  }

  async function createTutorialSession(previousPath) {
    const sessionPath = await fs.mkdtemp(path.join(tutorialRoot, 'picto-guided-tour-'));
    const libraryPath = path.join(sessionPath, 'Guided Tour.library');
    await fs.mkdir(path.join(libraryPath, 'blobs'), { recursive: true });
    return { path: libraryPath, sessionPath, previousPath };
  }

  /**
   * Opens and seeds the tour library of `session` under `attempt`, ending
   * the session when that fails. A superseded or cancelled tour rejects with
   * the cancellation sentinel, so that the renderer never runs tour steps
   * against whatever library is open instead; a failed one re-opens the
   * previous library under the same attempt and rejects with the tour error.
   * `abandoned` lists every session whose directory is disposable then.
   */
  async function openTutorialSession(attempt, session, abandoned = [session]) {
    try {
      sendToAllWindows('library-switching', { path: session.path, tutorial: true });
      await closeUnderAttempt(attempt);
      await startNativeOpen(attempt, () => openTutorialLibrary(session.path, tutorialFixtureRoot));
      ensureOwned(attempt);
      // Every seed command re-checks ownership: a takeover (a cloud join is
      // not gated by the tour) must not have the rest of the seed land in
      // whatever library is open by then.
      await seedTutorialLibrary(() => ensureOwned(attempt));
      publishOpen(attempt, { tutorial: true });
      buildAppMenu();
      return { path: session.path };
    } catch (error) {
      const outcome = settleFailedOpen(attempt, error);
      if (outcome === 'shutdown') throw error;
      if (abandoned.includes(tutorialSession)) tutorialSession = null;
      for (const entry of abandoned) await removeTutorialSessionDirectory(entry);
      if (outcome === 'superseded') throw new Error(LIBRARY_OPEN_CANCELLED);
      if (outcome === 'cancelled') throw error;
      if (await reopenUnderAttempt(attempt, session.previousPath, { restored: true }) === 'superseded') {
        throw new Error(LIBRARY_OPEN_CANCELLED);
      }
      throw error;
    }
  }

  async function startTutorialLibrary() {
    if (tutorialSession) return { path: tutorialSession.path };
    // A tour started while the previous one is still being restored returns
    // to the same user library, never to the tour directory being removed.
    const previousPath = finishingSession?.previousPath ?? getCurrentLibraryRoot();
    if (!previousPath) throw new Error('Open a library before starting the guided tour');
    await cleanupStaleTutorialLibraries();
    const session = await createTutorialSession(previousPath);
    tutorialSession = session;
    return runAttempt(session.path, (attempt) => openTutorialSession(attempt, session));
  }

  async function resetTutorialLibrary() {
    if (!tutorialSession) throw new Error('The guided tour is not running');
    const previous = tutorialSession;
    const session = await createTutorialSession(previous.previousPath);
    tutorialSession = session;
    return runAttempt(session.path, async (attempt) => {
      const result = await openTutorialSession(attempt, session, [previous, session]);
      await removeTutorialSessionDirectory(previous);
      return result;
    });
  }

  async function seedTutorialLibrary(beforeCommand = () => {}) {
    const command = async (name, args) => {
      beforeCommand();
      return JSON.parse(await invokeSerialized(name, args));
    };
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
    // The tour is over now: switching is allowed again and supersedes this
    // restore, which then touches nothing (the directory is removed by the
    // next cleanup).
    tutorialSession = null;
    finishingSession = session;
    try {
      return await runAttempt(session.previousPath, async (attempt) => {
        try {
          sendToAllWindows('library-switching', { path: session.previousPath, tutorial: false });
          await closeUnderAttempt(attempt);
          await startNativeOpen(attempt, () => openLibrary(session.previousPath));
          publishOpen(attempt, { restored: true });
          buildAppMenu();
        } catch (error) {
          if (settleFailedOpen(attempt, error) !== 'superseded') await removeTutorialSessionDirectory(session);
          throw error;
        }
        // Published first: a removal failing (EBUSY on Windows) must not
        // leave the open library unpublished.
        await removeTutorialSessionDirectory(session);
        return { restored: true, path: session.previousPath };
      }, { onSuperseded: () => ({ restored: false, path: session.previousPath }) });
    } finally {
      if (finishingSession === session) finishingSession = null;
    }
  }

  function getTutorialSession() {
    return tutorialSession ? { active: true, path: tutorialSession.path } : { active: false };
  }

  async function createLibrary({ name, savePath }) {
    const cleanName = String(name ?? '').trim();
    if (!cleanName) throw new Error('Library name cannot be empty');
    if (/[/\\]/.test(cleanName)) throw new Error('Library name cannot contain slashes');
    if (isInsideLibraryPackage(path, savePath)) {
      throw new Error('A library cannot be created inside another Picto library');
    }
    const libraryPath = path.join(savePath, `${cleanName}.library`);
    await fs.mkdir(libraryPath, { recursive: true });
    await fs.mkdir(path.join(libraryPath, 'blobs'), { recursive: true });
    // Listed as soon as it exists on disk: a switch superseded before it
    // publishes must not leave the new library unlisted. The switch lists
    // it again once it publishes.
    await recordLibrary(libraryPath, () => addLibraryToHistory(libraryPath));
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
    if (isInsideLibraryPackage(path, picked.filePaths[0])) {
      throw new Error('A library cannot be created inside another Picto library');
    }
    const targetRoot = path.join(picked.filePaths[0], `${cleanName}.library`);
    if (await fs.access(targetRoot).then(() => true, () => false)) {
      if (await isFailedCloudJoinDirectory(fs, targetRoot)) {
        await fs.rm(targetRoot, { recursive: true, force: true });
      } else {
        throw new Error(`A library named "${cleanName}" already exists at that location`);
      }
    }

    const previousPath = getCurrentLibraryRoot();
    libraryFailure = null;
    // The native join closes the previous library itself, restores it when
    // the join fails, and leaves nothing open when its open is cancelled.
    let joined = null;
    // Set once the joined library is valid on disk and listed without being
    // published: it can be opened from Library Manager.
    let listed = false;
    const removeRemnants = () => fs.rm(targetRoot, { recursive: true, force: true }).catch(() => {});
    const listJoinedLibrary = async () => {
      await recordLibrary(targetRoot, async () => {
        await addLibraryToHistory(targetRoot);
        await setLibraryMeta(targetRoot, { cloudLibraryId: libraryId });
      });
      buildAppMenu();
    };
    return runAttempt(targetRoot, async (attempt) => {
      try {
        sendToAllWindows('library-switching', { path: targetRoot });
        const serialized = await startNativeOpen(attempt, () => invokeSerialized('cloud.library.join', {
          provider,
          account_label: accountLabel,
          root_path: rootPath,
          library_id: libraryId,
          target_root: targetRoot,
        }));
        joined = JSON.parse(serialized);
        ensureOwned(attempt);
        await applyPlatformLibraryIcon(targetRoot);
        // Published first: the library is open once the native join
        // completed, and the bookkeeping that follows cannot make it not open.
        publishOpen(attempt);
        await listJoinedLibrary();
        return joined;
      } catch (error) {
        const outcome = settleFailedOpen(attempt, error);
        if (outcome === 'failed') {
          // A failed join leaves only remnants worth removing: the native
          // join sets its database aside and restores the previous library
          // itself. The windows learn of the failure before the previous
          // library is back.
          if (!joined) await removeRemnants();
          reportSettledFailure(attempt, error);
          if (previousPath) sendToAllWindows('library-switched', { path: previousPath });
          throw error;
        }
        // Superseded, cancelled, or shut down. The joined library is valid
        // on disk when the join completed natively, and also when the
        // sentinel interrupted it: the native join holds the invocation
        // write lock through its download, so a cancel can only hit its
        // final open, after which the database is complete at the target.
        // Left unlisted, that library would refuse every later join to its
        // folder ("already exists"). A sentinel with no database (the open
        // was refused before it started) or a native failure underneath a
        // takeover leaves only staging remnants, which would block every
        // later join to that folder as well.
        if (joined || (isLibraryOpenCancelled(error) && await isValidLibrary(targetRoot))) {
          listed = true;
          await listJoinedLibrary();
        } else {
          await removeRemnants();
        }
        // A cancelled open leaves nothing open; nothing is published.
        if (outcome === 'cancelled') setCurrentLibraryRoot(null);
        throw error;
      }
    }, {
      // The joined library exists (closed by the newer attempt) and is
      // listed so that it can be opened from Library Manager; nothing is
      // published. Without the native result, the caller learns the path
      // and the library it joined.
      onSuperseded: () => (listed
        ? { ...(joined ?? { library_id: libraryId, path: targetRoot }), opened: false }
        : null),
    });
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
    const deletionPath = await ownedLibraryPath(libraryPath);
    while (true) {
      const currentPath = getCurrentLibraryRoot();
      const pendingPath = openingPath();
      for (const activePath of [currentPath, pendingPath]) {
        if (!activePath) continue;
        const resolved = await fs.realpath(activePath).catch(() => path.resolve(activePath));
        if (resolved === deletionPath) {
          throw new Error('Cannot delete the currently open or opening library. Switch to a different library first.');
        }
      }
      // Opening can settle or change during realpath; validate the new state before unlinking.
      if (currentPath === getCurrentLibraryRoot() && pendingPath === openingPath()) break;
    }
    if (deletingLibraries.has(deletionPath)) throw new Error('Library deletion is already in progress.');
    deletingLibraries.add(deletionPath);
    try {
      await fs.rm(deletionPath, { recursive: true, force: false });
    } finally {
      deletingLibraries.delete(deletionPath);
    }
    await recordLibrary(libraryPath, () => removeLibraryFromHistory(libraryPath));
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
      let renamed = false;
      return runAttempt(newPath, async (attempt) => {
        sendToAllWindows('library-switching', { path: newPath });
        await closeUnderAttempt(attempt);
        ensureOwned(attempt);
        try {
          await fs.rename(libraryPath, newPath);
        } catch (err) {
          const renameError = new Error(`Failed to rename library: ${err.message}`);
          await reopenUnderAttempt(attempt, libraryPath);
          throw renameError;
        }
        // The disk rename happened: the library exists under its new name
        // whatever happens next. Recording it in the history is bookkeeping
        // that can only be logged, never turned into an open failure.
        renamed = true;
        await recordLibrary(newPath, () => updateLibraryPath(libraryPath, newPath));
        await startNativeOpen(attempt, () => openLibrary(newPath));
        publishOpen(attempt);
        buildAppMenu();
        return { newPath };
      }, {
        // Superseded after the rename reached the disk, the library exists
        // under its new name; before that, the caller learns why nothing
        // was renamed.
        onSuperseded: (error) => {
          if (renamed) return { newPath };
          throw error;
        },
      });
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

    // A library that still exists is moved into the chosen folder. A missing
    // one is located: the chosen folder must already contain it.
    const picked = await dialog.showOpenDialog({
      title: oldExists ? `Choose destination for "${name}"` : `Locate "${name}"`,
      properties: ['openDirectory', 'createDirectory'],
      message: oldExists
        ? `${basename} will be placed in the selected folder`
        : `Choose the folder that contains ${basename}`,
    });
    if (picked.canceled || picked.filePaths.length === 0) {
      return { action: 'cancelled' };
    }

    const pickedDir = picked.filePaths[0];
    if (oldExists && isInsideLibraryPackage(path, pickedDir)) {
      throw new Error('A library cannot be moved inside another Picto library.');
    }
    const destDir = oldExists ? await fs.realpath(pickedDir) : pickedDir;
    if (oldExists && isInsideLibraryPackage(path, destDir)) {
      throw new Error('A library cannot be moved inside another Picto library.');
    }
    const newPath = oldExists
      ? path.join(destDir, basename)
      : await locatedLibraryPath(destDir, basename);
    if (newPath === oldPath) return { action: 'cancelled' };

    const destExists = newPath && await fs.access(newPath).then(() => true, () => false);
    if (!oldExists) {
      if (!newPath) {
        await dialog.showMessageBox({
          type: 'error',
          title: 'Library Not Found',
          message: `"${basename}" was not found in that folder.`,
        });
        return { action: 'cancelled' };
      }
    } else if (destExists) {
      await dialog.showMessageBox({
        type: 'error',
        title: 'Already Exists',
        message: `"${basename}" already exists at that location.`,
      });
      return { action: 'cancelled' };
    }

    if (oldExists) {
      if (oldPath === getCurrentLibraryRoot()) {
        let moved = false;
        return runAttempt(newPath, async (attempt) => {
          sendToAllWindows('library-switching', { path: newPath });
          await closeUnderAttempt(attempt);
          ensureOwned(attempt);
          try {
            await moveLibraryDirectory(oldPath, newPath);
          } catch (err) {
            const moveError = new Error(`Failed to move library: ${err.message}`);
            await reopenUnderAttempt(attempt, oldPath);
            throw moveError;
          }
          // The move happened: the library exists at its new place whatever
          // happens next. Recording it in the history is bookkeeping that
          // can only be logged, never turned into an open failure.
          moved = true;
          await recordLibrary(newPath, () => updateLibraryPath(oldPath, newPath));
          await startNativeOpen(attempt, () => openLibrary(newPath));
          publishOpen(attempt);
          buildAppMenu();
          return { action: 'relocated', newPath };
        }, {
          // Superseded after the move reached the disk, the library exists
          // at its new place; before that, the caller learns why nothing
          // was moved.
          onSuperseded: (error) => {
            if (moved) return { action: 'relocated', newPath };
            throw error;
          },
        });
      } else {
        await moveLibraryDirectory(oldPath, newPath);
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
      openingPath: openingPath(),
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
    // A report that arrives while an open is in flight, or while no library
    // is current, is stale: it comes from a window of the library that a
    // switch just closed (its reads fail with "No library is open"), not
    // from the library being opened. Acting on it would cancel the newer
    // open, or blame a library that already failed, and open the chooser
    // a second time.
    const failedPath = getCurrentLibraryRoot();
    if (openingPath() !== null || !failedPath) return null;
    const failureMessage = String(message || 'The library navigation could not be loaded.');
    // A switch started during the close below clears the shared failure;
    // the windows still learn of this one.
    const failure = { path: failedPath, message: failureMessage };
    libraryFailure = failure;
    try {
      await closeLibrary();
    } catch (error) {
      console.error('[library] failed to close after an initial read failure', error);
    }
    // A switch that started during that close owns the UI now (it may even
    // have published its library): this report describes a library that is
    // no longer current, so it must neither clear the root nor reach windows.
    if (openingPath() !== null || getCurrentLibraryRoot() !== failedPath) return null;
    setCurrentLibraryRoot(null);
    buildAppMenu();
    sendToAllWindows('library-open-failed', failure);
    return failure;
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
    return runAttempt(libraryPath, async (attempt) => {
      sendToAllWindows('library-switching', { path: libraryPath });
      await startNativeOpen(attempt, () => openLibrary(libraryPath));
      // Published first: the library is open once its native open completed,
      // and the bookkeeping that follows cannot make it not open.
      publishOpen(attempt);
      if (remember) await recordLibrary(libraryPath, () => addLibraryToHistory(libraryPath));
    }, { onSuperseded: () => undefined });
  }

  return {
    createLibrary,
    deleteLibrary,
    getLibraryConfig,
    handleMissingLibrary,
    initializeInitialLibrary,
    isShuttingDown,
    isValidLibrary,
    libraryDisplayName,
    joinCloudLibrary,
    markShuttingDown,
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
