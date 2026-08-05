import { app, BrowserWindow, WebContentsView, clipboard, dialog, ipcMain, Menu, nativeImage, nativeTheme, protocol, screen } from 'electron';
import fs from 'node:fs/promises';
import fsModule from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { initRuntime, initialize, invoke, onNativeEvent, openLibrary, closeLibrary, startNativeDrag } from './nativeClient.mjs';
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
if (isPackagedSmoke && process.env.PICTO_SMOKE_APP_DATA) {
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

// Single instance guard — prevent multiple Picto processes from running.
// If another instance is already running, focus its window and quit this one.
const gotLock = isPackagedSmoke || app.requestSingleInstanceLock();
if (!gotLock) {
  app.quit();
}

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const isDev = !app.isPackaged;
if (isDev && process.platform === 'darwin') {
  // Native hot reload launches a replacement Electron process. Accessory mode
  // keeps that process launch from activating Picto over the user's editor.
  app.setActivationPolicy('accessory');
}
const DEV_URL = process.env.VITE_DEV_SERVER_URL || 'http://127.0.0.1:8080';
const SMOKE_SETTLE_MS = 1500;
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
    await closeNativeLibraryOnce();
    nativeShutdownSettled = true;
    if (smokeFailureReported) return;
    reportPackagedSmoke('native-library-closed');
    app.exit(0);
  } catch (error) {
    nativeShutdownSettled = true;
    failPackagedSmoke('shutdown-failed', { message: error?.message ?? String(error) }, false);
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
  isSmoke: isPackagedSmoke,
  sendToAllWindows: (...args) => windowManager.sendToAllWindows(...args),
});

let buildAppMenu = () => {};

const libraryHost = createLibraryHostService({
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

    if (name === 'open-detail-window') {
      try {
        const data = typeof payload === 'string' ? JSON.parse(payload) : payload;
        const hash = data?.hash;
        if (isValidHash(hash)) {
          const label = `detail-${hash.slice(0, 12)}`;
          const existing = windowManager.getWindow(label);
          if (existing && !existing.isDestroyed()) {
            existing.focus();
          } else {
            const size = windowManager.calcDetailWindowSize(data?.width ?? 0, data?.height ?? 0);
            windowManager.createWindow(label, hash, size.width, size.height);
          }
        }
      } catch {}
      return;
    }

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
