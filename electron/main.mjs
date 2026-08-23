import { app, BrowserWindow, WebContentsView, clipboard, dialog, ipcMain, Menu, nativeImage, nativeTheme, protocol, screen } from 'electron';
import fs from 'node:fs/promises';
import fsModule from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { initRuntime, invoke, onNativeEvent, openLibrary, closeLibrary, startNativeDrag } from './nativeClient.mjs';
import {
  addLibraryToHistory,
  getCachedConfig,
  loadGlobalConfig,
  removeLibraryFromHistory,
  saveGlobalConfig,
  togglePinned,
  updateLibraryPath,
} from './globalConfig.mjs';
import { createMediaProtocolService, isValidHash } from './protocol/media.mjs';
import { createWindowManager } from './windows/windowManager.mjs';
import { setMainWindow } from './services/logForwarder.mjs';
import { createMenuManager } from './windows/menu.mjs';
import { createLibraryHostService } from './services/libraryHostService.mjs';
import { registerIpcHandlers } from './ipc/registerHandlers.mjs';
import { createAutoUpdaterService } from './services/autoUpdater.mjs';

const isPackagedSmoke = process.env.PICTO_PACKAGED_SMOKE === '1';
const isE2E = process.env.PICTO_E2E === '1';
const isAutomation = isPackagedSmoke || isE2E;
if (isAutomation && process.env.PICTO_SMOKE_APP_DATA) {
  app.setPath('appData', process.env.PICTO_SMOKE_APP_DATA);
  app.setPath('userData', path.join(process.env.PICTO_SMOKE_APP_DATA, 'user-data'));
}
app.commandLine.appendSwitch('disable-features', 'AutofillServerCommunication');
app.commandLine.appendSwitch('force_high_performance_gpu');
if (process.env.PICTO_EXPERIMENTAL_GPU_FLAGS === '1') {
  app.commandLine.appendSwitch('enable-gpu-rasterization');
  app.commandLine.appendSwitch('enable-zero-copy');
  app.commandLine.appendSwitch('num-raster-threads', '4');
}

const isDev = !app.isPackaged;
if (isDev && process.platform === 'darwin') {
  app.setActivationPolicy('accessory');
}

// Single instance guard — prevent multiple Picto processes from running.
// If another instance is already running, focus its window and quit this one.
const gotLock = isAutomation || app.requestSingleInstanceLock();
if (!gotLock) {
  app.quit();
}

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const DEV_URL = process.env.VITE_DEV_SERVER_URL || 'http://127.0.0.1:8080';
const SMOKE_SETTLE_MS = 1500;
const PACKAGED_SYNC_SMOKE_LIBRARY = 'packaged-sync-smoke';
let nativeClosePromise = null;
let nativeShutdownSettled = false;
let nativeQuitPromise = null;
let smokeFailureReported = false;
let smokeExitRequested = false;

function reportPackagedSmoke(event, details = {}) {
  if (!isPackagedSmoke) return;
  process.stdout.write(`[picto-packaged-smoke] ${JSON.stringify({ event, ...details })}\n`);
}

function closeNativeLibraryOnce() {
  nativeClosePromise ??= closeLibrary();
  return nativeClosePromise;
}

async function waitForSmokeMedia(hash, timeoutMs = 10_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const mediaPath = await invoke('resolve_file_path', { hash });
      const bytes = await fs.readFile(mediaPath);
      if (createHash('sha256').update(bytes).digest('hex') !== hash) {
        throw new Error('resolved smoke media failed its SHA-256 check');
      }
      return mediaPath;
    } catch (error) {
      if (String(error).includes('SHA-256')) throw error;
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
  }
  throw new Error(`smoke media ${hash} did not become available within ${timeoutMs}ms`);
}

async function runPackagedSyncSmoke() {
  const shareRoot = process.env.PICTO_SMOKE_SYNC_ROOT;
  const phase = process.env.PICTO_SMOKE_SYNC_PHASE;
  const mediaPath = process.env.PICTO_SMOKE_MEDIA_PATH;
  const mediaHash = process.env.PICTO_SMOKE_MEDIA_HASH;
  if (!shareRoot || !phase || !mediaPath || !mediaHash) return;

  if (phase === 'publish') {
    await invoke('add_media', { paths: [mediaPath], initial_status: 1 });
    await waitForSmokeMedia(mediaHash);
    const created = await invoke('create_folder', { name: 'From device A' });
    const result = await invoke('sync_create_remote_library', {
      share_root: shareRoot,
      name: PACKAGED_SYNC_SMOKE_LIBRARY,
    });
    const status = await invoke('sync_get_status', {});
    reportPackagedSmoke('sync-device-a-published', {
      device_id: status.device_id,
      source_folder_id: created.folder_id,
      segments_uploaded: result.report.segments_uploaded,
    });
    return;
  }

  if (phase === 'peer') {
    const result = await invoke('sync_connect_remote_library', {
      share_root: shareRoot,
      name: PACKAGED_SYNC_SMOKE_LIBRARY,
    });
    const peerTree = await invoke('get_sidebar_tree', {});
    const peerFolder = peerTree.nodes.find(
      (node) => node.kind === 'folder' && node.name === 'From device A',
    );
    if (!peerFolder) throw new Error('device B did not receive the folder created on device A');
    await waitForSmokeMedia(mediaHash);

    const peerFolderId = Number(peerFolder.id.replace(/^folder:/, ''));
    if (!Number.isSafeInteger(peerFolderId)) throw new Error('device B received an invalid folder identity');
    await invoke('update_folder', { folder_id: peerFolderId, name: 'Renamed on device B' });
    const publish = await invoke('sync_now', {});
    const status = await invoke('sync_get_status', {});
    reportPackagedSmoke('sync-device-b-published', {
      device_id: status.device_id,
      initial_ops_applied: result.report.ops_applied,
      segments_uploaded: publish.report.segments_uploaded,
    });
    return;
  }

  if (phase === 'verify') {
    const startupStatus = await invoke('sync_get_status', {});
    const result = await invoke('sync_now', {});
    await waitForSmokeMedia(mediaHash);
    const sourceTree = await invoke('get_sidebar_tree', {});
    const converged = sourceTree.nodes.some(
      (node) => node.kind === 'folder' && node.name === 'Renamed on device B',
    );
    if (!converged) throw new Error('device A did not receive the folder rename from device B');
    const finalStatus = await invoke('sync_get_status', {});
    if (
      finalStatus.waiting_for_prerequisites
      || finalStatus.more_remote_work
      || finalStatus.pending_remote_ops !== 0
      || finalStatus.missing_blobs !== 0
      || finalStatus.failed_blobs !== 0
      || !finalStatus.last_success_at
    ) {
      throw new Error(`sync did not settle cleanly: ${JSON.stringify(finalStatus)}`);
    }
    reportPackagedSmoke('two-device-sync-complete', {
      device_id: startupStatus.device_id,
      startup_ops_applied: startupStatus.last_report?.ops_applied ?? 0,
      return_ops_applied: result.report.ops_applied,
      media_hash: mediaHash,
    });
    return;
  }

  throw new Error(`unknown packaged sync smoke phase: ${phase}`);
}

function failPackagedSmoke(event, details = {}, closeNative = true) {
  reportPackagedSmoke(event, details);
  if (!isPackagedSmoke || smokeFailureReported) return;

  smokeFailureReported = true;
  if (!closeNative) {
    app.exit(1);
    return;
  }
  void closeNativeLibraryOnce()
    .catch((error) => {
      reportPackagedSmoke('shutdown-failed', { message: error?.message ?? String(error) });
    })
    .finally(() => {
      nativeShutdownSettled = true;
      app.exit(1);
    });
}

async function completePackagedSmoke() {
  if (!isPackagedSmoke || smokeExitRequested) return;
  smokeExitRequested = true;
  try {
    await runPackagedSyncSmoke();
    await closeNativeLibraryOnce();
    nativeShutdownSettled = true;
    if (smokeFailureReported) return;
    reportPackagedSmoke('native-library-closed');
    app.exit(0);
  } catch (error) {
    failPackagedSmoke('sync-smoke-failed', { message: error?.message ?? String(error) });
  }
}

function awaitNativeShutdownBeforeQuit(event) {
  if (nativeShutdownSettled) return;
  event.preventDefault();
  nativeQuitPromise ??= closeNativeLibraryOnce()
    .then(() => {
      nativeShutdownSettled = true;
      app.quit();
    })
    .catch((error) => {
      nativeShutdownSettled = true;
      console.error('[main] native library shutdown failed:', error);
      if (isPackagedSmoke) {
        reportPackagedSmoke('shutdown-failed', { message: error?.message ?? String(error) });
      }
      app.exit(1);
    });
}

app.on('before-quit', awaitNativeShutdownBeforeQuit);
app.on('will-quit', awaitNativeShutdownBeforeQuit);
if (isDev) {
  process.once('SIGTERM', () => app.quit());
  process.once('SIGINT', () => app.quit());
}

if (isDev && process.env.PICTO_REMOTE_DEBUGGING !== '0') {
  // CDP endpoint for dev tooling (electron-mcp-server, screenshots, eval).
  app.commandLine.appendSwitch('remote-debugging-port', '9222');
}

app.setName('Picto');
if (process.platform === 'win32') {
  app.setAppUserModelId('com.picto.desktop');
}

if (app.isPackaged) {
  // macOS: extraFiles land in Contents/, but the exe is in Contents/MacOS/
  const sidecarBase = process.platform === 'darwin'
    ? path.join(path.dirname(process.execPath), '..')
    : path.dirname(process.execPath);
  process.env.PICTO_FFMPEG_DIR = sidecarBase;
  process.env.PICTO_GALLERY_DL_DIR = path.join(sidecarBase, 'gallery-dl');
}

protocol.registerSchemesAsPrivileged([
  { scheme: 'media', privileges: { standard: true, secure: true, supportFetchAPI: true, stream: true, corsEnabled: true } },
]);

let currentLibraryRoot = null;
const getCurrentLibraryRoot = () => currentLibraryRoot;
const setCurrentLibraryRoot = (nextRoot) => {
  currentLibraryRoot = nextRoot;
};

const mediaProtocol = createMediaProtocolService({
  protocol,
  path,
  invoke,
  isDev,
  getCurrentLibraryRoot,
});


const windowManager = createWindowManager({
  BrowserWindow,
  WebContentsView,
  screen,
  path,
  __dirname,
  DEV_URL,
  isDev,
  getCachedConfig,
  saveGlobalConfig,
  onWindowEvent: (event, details) => {
    if (event === 'did-finish-load' && details.label === 'main') {
      reportPackagedSmoke(event, details);
      setTimeout(() => {
        reportPackagedSmoke('settle-complete');
        void completePackagedSmoke();
      }, SMOKE_SETTLE_MS);
      return;
    }
    if (event !== 'did-finish-load') failPackagedSmoke(event, details);
  },
});

const updaterService = createAutoUpdaterService({
  app,
  isDev,
  isSmoke: isAutomation,
  sendToAllWindows: (...args) => windowManager.sendToAllWindows(...args),
});

let buildAppMenu = () => {};

const libraryHost = createLibraryHostService({
  fs,
  path,
  dialog,
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
  createMainWindow: () => {
    const existing = windowManager.getWindow('main');
    if (existing && !existing.isDestroyed()) {
      existing.focus();
      return existing;
    }
    return windowManager.createWindow('main');
  },
  sendToAllWindows: windowManager.sendToAllWindows,
  buildAppMenu: () => buildAppMenu(),
});

const menuManager = createMenuManager({
  app,
  Menu,
  dialog,
  isDev,
  getCachedConfig,
  saveGlobalConfig,
  getCurrentLibraryRoot,
  libraryDisplayName: libraryHost.libraryDisplayName,
  switchLibrary: libraryHost.switchLibrary,
  openSettingsWindow: windowManager.openSettingsWindow,
  openSubscriptionsWindow: windowManager.openSubscriptionsWindow,
  openLibraryManager: windowManager.openLibraryManager,
  sendToFocusedWindow: windowManager.sendToFocusedWindow,
  sendToMainWindow: windowManager.sendToMainWindow,
});
buildAppMenu = menuManager.buildAppMenu;

if (isPackagedSmoke) {
  ipcMain.on('picto:smoke:renderer-failure', (_event, failure) => {
    const event = failure?.event === 'window-error' ? 'window-error' : 'unhandled-rejection';
    failPackagedSmoke(event, { message: failure?.message ?? 'renderer failure' });
  });
}

registerIpcHandlers({
  app,
  ipcMain,
  BrowserWindow,
  Menu,
  nativeImage,
  clipboard,
  dialog,
  nativeTheme,
  screen,
  invoke,
  isValidHash,
  buildBlobPath: mediaProtocol.buildBlobPath,
  windowManager,
  libraryService: libraryHost,
  updaterService,
  startNativeDrag,
});

function wireNativeEvents() {
  onNativeEvent((name, payload) => {
    if (!name || typeof name !== 'string') return;

    windowManager.sendToAllWindows(name, payload);
  });
}

async function resolveInitialLibrary(config) {
  if (process.env.PICTO_LIBRARY_ROOT) {
    return process.env.PICTO_LIBRARY_ROOT;
  }
  if (config.lastLibrary && await libraryHost.isValidLibrary(config.lastLibrary)) {
    return config.lastLibrary;
  }
  for (const libraryPath of config.libraryHistory) {
    if (await libraryHost.isValidLibrary(libraryPath)) {
      return libraryPath;
    }
  }
  return null;
}

async function bootstrapApplication() {
  console.info('[main] app.whenReady begin');
  initRuntime();
  await mediaProtocol.registerMediaProtocol();
  console.info('[main] media protocol registered');
  console.info('[main] IPC handlers registered');

  wireNativeEvents();

  const config = await loadGlobalConfig();
  console.info('[main] global config loaded');

  let libraryToOpen = await resolveInitialLibrary(config);

  if (!libraryToOpen && config.lastLibrary) {
    const result = await libraryHost.handleMissingLibrary(config.lastLibrary);
    if (result.action === 'relocated') {
      libraryToOpen = result.newPath;
    }
  }

  if (libraryToOpen) {
    console.info('[main] initializing library', { libraryToOpen });
    await libraryHost.initializeInitialLibrary(libraryToOpen);
    reportPackagedSmoke('native-library-initialized');
    console.info('[main] library initialized in native core');
    console.info('[main] library history updated');
  } else {
    setCurrentLibraryRoot(null);
    console.info('[main] no initial library selected; starting without an open library');
  }

  // Check for updates before showing the window — if an update is found
  // within 3 seconds, download + install it (app restarts automatically).
  // If no update or timeout, proceed to show the app.
  await updaterService.checkAndUpdateOnStartup(3000);

  buildAppMenu();
  console.info('[main] app menu built');

  console.info('[main] creating main window');
  const mainWin = windowManager.createWindow('main');
  setMainWindow(mainWin);
  console.info('[main] main window creation requested');
}

process.on('uncaughtException', (err) => {
  failPackagedSmoke('uncaught-exception', { message: err?.message ?? String(err) });
  console.error('[main] Uncaught exception:', err);
});
process.on('unhandledRejection', (reason) => {
  failPackagedSmoke('unhandled-rejection', { message: reason?.message ?? String(reason) });
  console.error('[main] Unhandled promise rejection:', reason);
});

app.on('second-instance', () => {
  // Native hot reload can overlap the old and new Electron processes briefly.
  // Do not let that development-only overlap steal focus from the active app.
  if (isDev) return;
  // Another instance tried to launch — focus the existing main window.
  const win = windowManager.getWindow('main');
  if (win && !win.isDestroyed()) {
    if (win.isMinimized()) win.restore();
    win.focus();
  }
});

// Dev-only: capture the main window to a PNG when the trigger file is
// touched (lets tooling screenshot the app without macOS screen-recording
// permission). `touch /tmp/picto-capture-request` → /tmp/picto-capture.png
function startDevCaptureWatcher() {
  if (!isDev) return;
  const fsSync = fsModule;
  const trigger = '/tmp/picto-capture-request';
  try {
    fsSync.writeFileSync(trigger, '');
    fsSync.watch(trigger, async () => {
      try {
        const win = windowManager.getWindow('main');
        if (!win || win.isDestroyed()) return;
        const image = await win.webContents.capturePage();
        fsSync.writeFileSync('/tmp/picto-capture.png', image.toPNG());
        console.info('[main] dev capture written to /tmp/picto-capture.png');
      } catch (err) {
        console.error('[main] dev capture failed:', err);
      }
    });
  } catch {
    // non-fatal dev helper
  }
}

app.whenReady().then(async () => {
  await bootstrapApplication();
  startDevCaptureWatcher();

  // Auto theme: broadcast OS dark/light mode changes to all windows
  nativeTheme.on('updated', () => {
    const isDark = nativeTheme.shouldUseDarkColors;
    for (const win of BrowserWindow.getAllWindows()) {
      if (!win.isDestroyed()) {
        win.webContents.send('picto:os-theme-changed', { isDark });
      }
    }
  });

  app.on('activate', () => {
    if (windowManager.getAllWindows().length === 0) {
      windowManager.createWindow('main');
    }
  });
}).catch((err) => {
  failPackagedSmoke('bootstrap-failed', { message: err?.message ?? String(err) });
  console.error('[main] app.whenReady failed:', err);
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit();
});
