import { app, BrowserWindow, WebContentsView, clipboard, dialog, ipcMain, Menu, nativeImage, nativeTheme, net, Notification, protocol, screen } from 'electron';
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { copyFiles, getAssociatedApplications, initRuntime, invoke, invokeSerialized, onNativeEvent, openLibrary, openTutorialLibrary, closeLibrary, openWithApplication, setFileIcon, startNativeDrag } from './nativeClient.mjs';
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
import { installConsoleForwarding, setMainWindow } from './services/logForwarder.mjs';
import { createMenuManager } from './windows/menu.mjs';
import { createLibraryHostService } from './services/libraryHostService.mjs';
import { registerIpcHandlers } from './ipc/registerHandlers.mjs';
import { createFlashThumbnailService } from './services/flashThumbnailService.mjs';
import { createPdfThumbnailService } from './services/pdfThumbnailService.mjs';
import { createDocumentThumbnailService } from './services/documentThumbnailService.mjs';
import { createSiteIconService } from './services/siteIconService.mjs';
import { createUpdateService } from './services/updateService.mjs';
import { createSubscriptionNotificationService } from './services/subscriptionNotificationService.mjs';
import { associatedFilesFromArguments } from './services/associatedFileService.mjs';

installConsoleForwarding();

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
const gotLock = isAutomation || app.requestSingleInstanceLock();
if (!gotLock) {
  dialog.showErrorBox(
    'Picto is already running',
    'Close the existing Picto process before starting another one.',
  );
  app.quit();
}

let bootstrapComplete = false;
let incomingAssociatedFiles = associatedFilesFromArguments(process.argv, process.cwd());
const pendingPictoPackPaths = [];
let associatedFileDispatch = Promise.resolve();

function acceptAssociatedFiles(values, workingDirectory = process.cwd()) {
  const entries = associatedFilesFromArguments(values, workingDirectory);
  if (entries.length === 0) return;
  incomingAssociatedFiles.push(...entries);
  if (bootstrapComplete) void drainAssociatedFiles();
}

app.on('open-file', (event, filePath) => {
  event.preventDefault();
  acceptAssociatedFiles([filePath]);
});

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const DEV_URL = process.env.VITE_DEV_SERVER_URL || 'http://127.0.0.1:8080';
const SMOKE_SETTLE_MS = 1500;
let nativeClosePromise = null;
let nativeShutdownSettled = false;
let nativeQuitPromise = null;
let smokeFailureReported = false;
let smokeExitRequested = false;

function reportPackagedSmoke(event, details = {}) {
  if (!isPackagedSmoke) return;
  process.stdout.write(`[picto-packaged-smoke] ${JSON.stringify({ ...details, event })}\n`);
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
    failPackagedSmoke('packaged-smoke-failed', { message: error?.message ?? String(error) });
  }
}

async function verifyPackagedSmokeMediaPlayback() {
  const hash = process.env.PICTO_SMOKE_MEDIA_HASH;
  if (!isValidHash(hash)) throw new Error('Packaged smoke media hash is missing or invalid.');
  const smokeWindow = new BrowserWindow({
    show: false,
    webPreferences: {
      backgroundThrottling: false,
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });
  try {
    const source = `media://localhost/file/${hash}.webm`;
    const markup = `<video id="media" muted src="${source}"></video>`;
    await smokeWindow.loadURL(`data:text/html,${encodeURIComponent(markup)}`);
    const result = await smokeWindow.webContents.executeJavaScript(`new Promise((resolve) => {
      const media = document.querySelector('#media');
      let settled = false;
      const finish = (event) => {
        if (settled) return;
        settled = true;
        resolve({
          event,
          currentTime: media.currentTime,
          duration: media.duration,
          readyState: media.readyState,
          errorCode: media.error?.code ?? null,
          errorMessage: media.error?.message ?? null,
        });
      };
      media.addEventListener('loadedmetadata', () => media.play().catch(() => {}), { once: true });
      media.addEventListener('timeupdate', () => {
        if (media.currentTime > 0.05) finish('playing');
      });
      media.addEventListener('error', () => finish('error'), { once: true });
      setTimeout(() => finish('timeout'), 10000);
      media.load();
    })`, true);
    if (result.event !== 'playing') {
      throw new Error(`Packaged WebM playback ${result.event}: ${result.errorMessage ?? `media error ${result.errorCode ?? 'unknown'}`}`);
    }
    reportPackagedSmoke('media-playback-ready', result);
  } finally {
    if (!smokeWindow.isDestroyed()) smokeWindow.destroy();
  }
}

async function verifyPackagedSmokeFlashPlayback() {
  const hash = process.env.PICTO_SMOKE_FLASH_HASH;
  if (!isValidHash(hash)) throw new Error('Packaged smoke Flash hash is missing or invalid.');
  const outputPath = path.join(app.getPath('temp'), `picto-flash-smoke-${process.pid}.png`);
  try {
    const result = await flashThumbnail.render({
      sourceUrl: `media://localhost/file/${hash}.swf`,
      outputPath,
      settleMs: 250,
    });
    const output = await fs.stat(outputPath);
    if (output.size === 0) throw new Error('Packaged Flash playback produced an empty frame.');
    reportPackagedSmoke('flash-runtime-ready', { ...result, bytes: output.size });
  } finally {
    await fs.rm(outputPath, { force: true });
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
  const nativeToolBase = process.platform === 'darwin'
    ? path.join(path.dirname(process.execPath), '..')
    : path.dirname(process.execPath);
  process.env.PICTO_FFMPEG_DIR = nativeToolBase;
}

protocol.registerSchemesAsPrivileged([
  { scheme: 'media', privileges: { standard: true, secure: true, supportFetchAPI: true, stream: true, corsEnabled: true } },
]);

let currentLibraryRoot = null;
const getCurrentLibraryRoot = () => currentLibraryRoot;
const setCurrentLibraryRoot = (nextRoot) => {
  currentLibraryRoot = nextRoot;
};

const flashThumbnail = createFlashThumbnailService({
  BrowserWindow,
  app,
  path,
  isDev,
  devUrl: DEV_URL,
});
const pdfThumbnail = createPdfThumbnailService({
  BrowserWindow,
  app,
  path,
  isDev,
  devUrl: DEV_URL,
});
const documentThumbnail = createDocumentThumbnailService({
  BrowserWindow,
  app,
  path,
  isDev,
  devUrl: DEV_URL,
});
const siteIconService = createSiteIconService({
  cacheDirectory: path.join(app.getPath('userData'), 'site-icons'),
  fetchImpl: (...args) => net.fetch(...args),
});

const mediaProtocol = createMediaProtocolService({
  protocol,
  path,
  invoke,
  isDev,
  getCurrentLibraryRoot,
  getKnownLibraryRoots: () => getCachedConfig().libraryHistory ?? [],
  flashThumbnail,
  pdfThumbnail,
  documentThumbnail,
  fetchFile: (filePath, range) => net.fetch(pathToFileURL(filePath).href, {
    headers: range ? { Range: `bytes=${range.start}-${range.end}` } : undefined,
  }),
  onThumbnailReady: (fileHash) => {
    windowManager.sendToAllWindows('picto:thumbnail-changed', { fileHash });
  },
});


const windowManager = createWindowManager({
  BrowserWindow,
  WebContentsView,
  screen,
  path,
  invoke,
  __dirname,
  DEV_URL,
  isDev,
  getCachedConfig,
  saveGlobalConfig,
  onWindowEvent: (event, details) => {
    if (event === 'did-finish-load' && details.label === 'main') {
      reportPackagedSmoke(event, details);
      if (!isPackagedSmoke) return;
      setTimeout(() => {
        void verifyPackagedSmokeMediaPlayback().then(() => verifyPackagedSmokeFlashPlayback()).then(() => {
          reportPackagedSmoke('settle-complete');
          return completePackagedSmoke();
        }).catch((error) => {
          failPackagedSmoke('packaged-smoke-failed', { message: error?.message ?? String(error) });
        });
      }, SMOKE_SETTLE_MS);
      return;
    }
    if (event !== 'did-finish-load') failPackagedSmoke(event, details);
  },
});

const subscriptionNotifications = createSubscriptionNotificationService({
  Notification,
  app,
  invokeSerialized,
  getCurrentLibraryRoot,
});

let buildAppMenu = () => {};

const libraryHost = createLibraryHostService({
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
  tutorialRoot: app.getPath('temp'),
  tutorialFixtureRoot: app.isPackaged
    ? path.join(process.resourcesPath, 'tutorial')
    : path.join(__dirname, '..', 'resources', 'tutorial'),
  setFileIcon,
});

function focusMainWindow() {
  const win = windowManager.getWindow('main');
  if (!win || win.isDestroyed()) return;
  if (win.isMinimized()) win.restore();
  win.focus();
}

function enqueuePictoPack(pathToPack, notify = true) {
  if (!pendingPictoPackPaths.includes(pathToPack)) pendingPictoPackPaths.push(pathToPack);
  if (notify) windowManager.sendToMainWindow('picto:associated-file-queued');
}

function drainAssociatedFiles() {
  if (!bootstrapComplete || incomingAssociatedFiles.length === 0) return associatedFileDispatch;
  const entries = incomingAssociatedFiles;
  incomingAssociatedFiles = [];
  associatedFileDispatch = associatedFileDispatch.then(async () => {
    for (const entry of entries) {
      try {
        if (entry.kind === 'library') await libraryHost.switchLibrary(entry.path);
        else enqueuePictoPack(entry.path);
      } catch (error) {
        console.error(`[main] could not open associated ${entry.kind}`, error);
        dialog.showErrorBox(
          entry.kind === 'library' ? 'Could not open Picto Library' : 'Could not open Picto Pack',
          error?.message ?? String(error),
        );
      }
    }
    focusMainWindow();
  });
  return associatedFileDispatch;
}

function claimAssociatedPictoPack() {
  return pendingPictoPackPaths.shift() ?? null;
}

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
  checkForUpdates: () => {
    void updateService.check().then(() => windowManager.sendToMainWindow('menu:show-updates'));
  },
});
buildAppMenu = menuManager.buildAppMenu;

const updateService = createUpdateService({
  app,
  net,
  sendToAllWindows: windowManager.sendToAllWindows,
});

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
  invokeSerialized,
  isValidHash,
  buildBlobPath: mediaProtocol.buildBlobPath,
  setThumbnail: mediaProtocol.setThumbnail,
  regenerateThumbnail: mediaProtocol.regenerateThumbnail,
  windowManager,
  libraryService: libraryHost,
  siteIconService,
  menuManager,
  startNativeDrag,
  copyFiles,
  getAssociatedApplications,
  openWithApplication,
  isDev,
  updateService,
  claimAssociatedPictoPack,
});

function wireNativeEvents() {
  onNativeEvent((name, payload) => {
    if (!name || typeof name !== 'string') return;

    if (name === 'picto:thumbnail-changed' && isValidHash(payload?.fileHash)) {
      mediaProtocol.invalidateThumbnail(payload.fileHash);
    }
    subscriptionNotifications.handleNativeEvent(name, payload);
    windowManager.sendToAllWindows(name, payload);
  });
}

async function resolveInitialLibrary(config) {
  // Test library overrides must never leak into an ordinary development
  // restart and replace the user's active library.
  if (isAutomation && process.env.PICTO_LIBRARY_ROOT) {
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
  initRuntime(path.join(app.getPath('appData'), 'picto'));
  await mediaProtocol.registerMediaProtocol();
  console.info('[main] media protocol registered');
  console.info('[main] IPC handlers registered');

  wireNativeEvents();

  const config = await loadGlobalConfig();
  console.info('[main] global config loaded');

  const startupAssociatedFiles = incomingAssociatedFiles;
  incomingAssociatedFiles = [];
  const startupLibrary = startupAssociatedFiles.findLast((entry) => entry.kind === 'library') ?? null;
  for (const entry of startupAssociatedFiles) {
    if (entry.kind === 'picto-pack') enqueuePictoPack(entry.path, false);
  }

  let libraryToOpen = startupLibrary && await libraryHost.isValidLibrary(startupLibrary.path)
    ? startupLibrary.path
    : await resolveInitialLibrary(config);

  if (!libraryToOpen && config.lastLibrary) {
    const result = await libraryHost.handleMissingLibrary(config.lastLibrary);
    if (result.action === 'relocated') {
      libraryToOpen = result.newPath;
    }
  }

  if (libraryToOpen) {
    const rememberInitialLibrary = !(isAutomation && process.env.PICTO_LIBRARY_ROOT);
    console.info('[main] initializing library', { libraryToOpen });
    const opening = libraryHost.initializeInitialLibrary(libraryToOpen, {
      remember: rememberInitialLibrary,
    });
    console.info('[main] creating main window for library reconciliation');
    const mainWin = windowManager.createWindow('main');
    setMainWindow(mainWin);
    await opening;
    reportPackagedSmoke('native-library-initialized');
    console.info('[main] library initialized in native core');
    if (rememberInitialLibrary) console.info('[main] library history updated');
  } else {
    setCurrentLibraryRoot(null);
    console.info('[main] no initial library selected; starting without an open library');
  }

  buildAppMenu();
  console.info('[main] app menu built');

  if (!windowManager.getWindow('main')) {
    console.info('[main] creating main window');
    const mainWin = windowManager.createWindow('main');
    setMainWindow(mainWin);
    console.info('[main] main window creation requested');
  }

  if (startupLibrary && libraryToOpen !== startupLibrary.path) {
    dialog.showErrorBox('Could not open Picto Library', `The library is missing or invalid:\n${startupLibrary.path}`);
  }
  bootstrapComplete = true;
  void drainAssociatedFiles();
}

process.on('uncaughtException', (err) => {
  failPackagedSmoke('uncaught-exception', { message: err?.message ?? String(err) });
  console.error('[main] Uncaught exception:', err);
});
process.on('unhandledRejection', (reason) => {
  failPackagedSmoke('unhandled-rejection', { message: reason?.message ?? String(reason) });
  console.error('[main] Unhandled promise rejection:', reason);
});

app.on('second-instance', (_event, argv, workingDirectory) => {
  // Native hot reload can overlap the old and new Electron processes briefly.
  // Do not let that development-only overlap steal focus from the active app.
  if (isDev) return;
  acceptAssociatedFiles(argv, workingDirectory);
  focusMainWindow();
});

app.whenReady().then(async () => {
  await bootstrapApplication();
  updateService.start();
  void subscriptionNotifications.refresh();

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
