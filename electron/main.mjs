import { app, BrowserWindow, clipboard, dialog, ipcMain, Menu, nativeImage, nativeTheme, protocol, screen } from 'electron';
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { initialize, invoke, onNativeEvent, openLibrary, closeLibrary } from './nativeClient.mjs';
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
import { createMediaMaintenanceService } from './services/mediaMaintenance.mjs';
import { createWindowManager } from './windows/windowManager.mjs';
import { createMenuManager } from './windows/menu.mjs';
import { createLibraryHostService } from './services/libraryHostService.mjs';
import { registerIpcHandlers } from './ipc/registerHandlers.mjs';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const isDev = !app.isPackaged;
const DEV_URL = process.env.VITE_DEV_SERVER_URL || 'http://127.0.0.1:8080';

app.setName('Picto');
if (process.platform === 'win32') {
  app.setAppUserModelId('com.picto.desktop');
}

if (app.isPackaged) {
  process.env.PICTO_FFMPEG_DIR = path.dirname(process.execPath);
  process.env.PICTO_GALLERY_DL_DIR = path.join(path.dirname(process.execPath), 'gallery-dl');
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

const mediaMaintenance = createMediaMaintenanceService({
  invoke,
  isDev,
  getCurrentLibraryRoot,
});

const windowManager = createWindowManager({
  BrowserWindow,
  screen,
  path,
  __dirname,
  DEV_URL,
  isDev,
  getCachedConfig,
  saveGlobalConfig,
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
  startBlurhashBackfill: mediaMaintenance.startBlurhashBackfill,
  stopBlurhashBackfill: mediaMaintenance.stopBlurhashBackfill,
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
  openLibraryManager: windowManager.openLibraryManager,
  sendToFocusedWindow: windowManager.sendToFocusedWindow,
  sendToMainWindow: windowManager.sendToMainWindow,
});
buildAppMenu = menuManager.buildAppMenu;

registerIpcHandlers({
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
    console.info('[main] library initialized in native core');
    console.info('[main] library history updated');
  } else {
    setCurrentLibraryRoot(null);
    console.info('[main] no initial library selected; starting without an open library');
  }

  buildAppMenu();
  console.info('[main] app menu built');

  console.info('[main] creating main window');
  windowManager.createWindow('main');
  console.info('[main] main window creation requested');
}

process.on('uncaughtException', (err) => {
  console.error('[main] Uncaught exception:', err);
});
process.on('unhandledRejection', (reason) => {
  console.error('[main] Unhandled promise rejection:', reason);
});

app.whenReady().then(async () => {
  await bootstrapApplication();
  app.on('activate', () => {
    if (windowManager.getAllWindows().length === 0) {
      windowManager.createWindow('main');
    }
  });
}).catch((err) => {
  console.error('[main] app.whenReady failed:', err);
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit();
});
