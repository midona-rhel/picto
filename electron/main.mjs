import { app, BrowserWindow, clipboard, dialog, ipcMain, Menu, nativeImage, nativeTheme, protocol, screen, shell } from 'electron';
import crypto from 'node:crypto';
import fs from 'node:fs/promises';
import { createReadStream } from 'node:fs';
import { Readable } from 'node:stream';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { initialize, openLibrary, closeLibrary, invoke, onNativeEvent } from './nativeClient.mjs';
import { loadGlobalConfig, saveGlobalConfig, addLibraryToHistory, removeLibraryFromHistory, togglePinned, getCachedConfig, updateLibraryPath } from './globalConfig.mjs';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const isDev = !app.isPackaged;
const DEV_URL = process.env.VITE_DEV_SERVER_URL || 'http://127.0.0.1:8080';

// Force consistent app identity in dev and packaged runtime.
app.setName('Picto');
if (process.platform === 'win32') {
  app.setAppUserModelId('com.picto.desktop');
}

// Point the Rust core at bundled sidecar binaries.
// In packaged builds, electron-builder places them next to the executable.
if (app.isPackaged) {
  process.env.PICTO_FFMPEG_DIR = path.dirname(process.execPath);
  process.env.PICTO_GALLERY_DL_DIR = path.join(path.dirname(process.execPath), 'gallery-dl');
}

// Must be called before app.whenReady() — enables media:// for <img>, fetch(), etc.
protocol.registerSchemesAsPrivileged([
  { scheme: 'media', privileges: { standard: true, secure: true, supportFetchAPI: true, stream: true, corsEnabled: true } },
]);

const windowsByLabel = new Map();
let currentLibraryRoot = null;
const thumbEnsureInFlight = new Map();
let blurhashBackfillTimer = null;
let blurhashBackfillInFlight = false;

const BLURHASH_BACKFILL_BATCH = 96;
const BLURHASH_BACKFILL_ACTIVE_DELAY_MS = 750;
const BLURHASH_BACKFILL_IDLE_DELAY_MS = 5000;
const MAIN_WINDOW_DEFAULT_WIDTH = 1200;
const MAIN_WINDOW_DEFAULT_HEIGHT = 800;
const MAIN_WINDOW_MIN_WIDTH = 700;
const MAIN_WINDOW_MIN_HEIGHT = 500;
const WINDOW_STATE_SAVE_DEBOUNCE_MS = 180;

function rectsIntersect(a, b) {
  return (
    a.x < b.x + b.width &&
    a.x + a.width > b.x &&
    a.y < b.y + b.height &&
    a.y + a.height > b.y
  );
}

function normalizeMainWindowState(raw) {
  if (!raw || typeof raw !== 'object') return null;
  const x = Number(raw.x);
  const y = Number(raw.y);
  const width = Number(raw.width);
  const height = Number(raw.height);
  const maximized = Boolean(raw.maximized);
  if (!Number.isFinite(width) || !Number.isFinite(height)) return null;

  const safe = {
    x: Number.isFinite(x) ? Math.round(x) : null,
    y: Number.isFinite(y) ? Math.round(y) : null,
    width: Math.max(MAIN_WINDOW_MIN_WIDTH, Math.round(width)),
    height: Math.max(MAIN_WINDOW_MIN_HEIGHT, Math.round(height)),
    maximized,
  };

  if (safe.x == null || safe.y == null) {
    return safe;
  }

  const rect = { x: safe.x, y: safe.y, width: safe.width, height: safe.height };
  const displays = screen.getAllDisplays();
  const intersectsAnyDisplay = displays.some((d) => rectsIntersect(rect, d.workArea));
  if (!intersectsAnyDisplay) {
    // Stale/off-screen monitor position: keep size, let OS center window.
    safe.x = null;
    safe.y = null;
  }
  return safe;
}

function getSavedMainWindowState() {
  const cfg = getCachedConfig();
  return normalizeMainWindowState(cfg?.windowState?.main ?? null);
}

function queueSaveMainWindowState(win, timerRef) {
  if (!win || win.isDestroyed()) return;
  if (timerRef.value != null) clearTimeout(timerRef.value);
  timerRef.value = setTimeout(() => {
    timerRef.value = null;
    if (win.isDestroyed()) return;
    const normalBounds = win.getNormalBounds();
    const nextState = {
      x: normalBounds.x,
      y: normalBounds.y,
      width: normalBounds.width,
      height: normalBounds.height,
      maximized: win.isMaximized(),
    };
    const cfg = getCachedConfig();
    cfg.windowState = {
      ...(cfg.windowState ?? {}),
      main: nextState,
    };
    void saveGlobalConfig(cfg).catch((err) => {
      if (isDev) console.warn('[main] failed to save window state', err);
    });
  }, WINDOW_STATE_SAVE_DEBOUNCE_MS);
}

function isValidHash(value) {
  return typeof value === 'string' && value.length === 64 && /^[a-fA-F0-9]+$/.test(value);
}

function buildBlobPath(kind, hash, ext) {
  const root = currentLibraryRoot;
  if (!root) return '';
  const ab = hash.slice(0, 2);
  const cd = hash.slice(2, 4);
  if (kind === 'thumb') return path.join(root, 'blobs', 't', ab, cd, `${hash}.jpg`);
  return path.join(root, 'blobs', 'f', ab, cd, `${hash}.${ext}`);
}

/** Resolve original path, allowing extension mismatch recovery by hash scan. */
async function resolveOriginalPath(hash, extHint) {
  const root = currentLibraryRoot;
  if (!root) return '';
  const ab = hash.slice(0, 2);
  const cd = hash.slice(2, 4);
  const dir = path.join(root, 'blobs', 'f', ab, cd);
  const hinted = path.join(dir, `${hash}.${extHint}`);
  try { await fs.stat(hinted); return hinted; } catch {}
  try {
    const entries = await fs.readdir(dir);
    const prefix = `${hash}.`;
    const found = entries.find((name) => name.startsWith(prefix));
    if (found) return path.join(dir, found);
  } catch {}
  return hinted;
}

/** Resolve the actual thumbnail path (async), trying .jpg then .png. */
async function resolveThumbPath(hash) {
  const root = currentLibraryRoot;
  if (!root) return '';
  const ab = hash.slice(0, 2);
  const cd = hash.slice(2, 4);
  const dir = path.join(root, 'blobs', 't', ab, cd);
  const jpg = path.join(dir, `${hash}.jpg`);
  try { await fs.stat(jpg); return jpg; } catch {}
  const png = path.join(dir, `${hash}.png`);
  try { await fs.stat(png); return png; } catch {}
  return jpg; // default to jpg for ensure_thumbnail fallback
}

function parseMediaUrl(urlString) {
  const url = new URL(urlString);
  const parts = url.pathname.split('/').filter(Boolean);
  if (parts.length !== 2) return null;
  const [kind, segment] = parts;
  if (kind === 'thumb') {
    const match = segment.match(/^([a-fA-F0-9]{64})\.jpg$/);
    if (!match) return null;
    return { kind: 'thumb', hash: match[1], ext: 'jpg' };
  }
  if (kind === 'file') {
    const match = segment.match(/^([a-fA-F0-9]{64})\.([a-zA-Z0-9]+)$/);
    if (!match) return null;
    return { kind: 'file', hash: match[1], ext: match[2].toLowerCase() };
  }
  return null;
}

function extToMime(ext) {
  const m = {
    jpg: 'image/jpeg', jpeg: 'image/jpeg', png: 'image/png', gif: 'image/gif', webp: 'image/webp',
    bmp: 'image/bmp', tiff: 'image/tiff', tif: 'image/tiff', svg: 'image/svg+xml', avif: 'image/avif',
    heif: 'image/heif', heic: 'image/heif', jxl: 'image/jxl', ico: 'image/x-icon', psd: 'image/vnd.adobe.photoshop',
    mp4: 'video/mp4', webm: 'video/webm', mkv: 'video/x-matroska', mov: 'video/quicktime', flv: 'video/x-flv',
    avi: 'video/x-msvideo', flac: 'audio/flac', wav: 'audio/x-wav', pdf: 'application/pdf', epub: 'application/epub+zip',
  };
  return m[ext] || 'application/octet-stream';
}

function parseRange(range, size) {
  if (!range || !range.startsWith('bytes=')) return null;
  const spec = range.slice(6).split(',')[0].trim();
  if (spec.startsWith('-')) {
    const n = Number(spec.slice(1));
    if (!Number.isFinite(n) || n <= 0 || n > size) return null;
    return { start: size - n, end: size - 1 };
  }
  if (spec.endsWith('-')) {
    const start = Number(spec.slice(0, -1));
    if (!Number.isFinite(start) || start < 0 || start >= size) return null;
    return { start, end: size - 1 };
  }
  const [a, b] = spec.split('-', 2).map(Number);
  if (!Number.isFinite(a) || !Number.isFinite(b) || a < 0 || b < a || a >= size) return null;
  return { start: a, end: Math.min(b, size - 1) };
}

function scheduleBlurhashBackfill(nextDelayMs) {
  if (blurhashBackfillTimer != null) {
    clearTimeout(blurhashBackfillTimer);
    blurhashBackfillTimer = null;
  }
  blurhashBackfillTimer = setTimeout(runBlurhashBackfillTick, nextDelayMs);
}

function stopBlurhashBackfill() {
  if (blurhashBackfillTimer != null) {
    clearTimeout(blurhashBackfillTimer);
    blurhashBackfillTimer = null;
  }
  blurhashBackfillInFlight = false;
}

function startBlurhashBackfill() {
  stopBlurhashBackfill();
  scheduleBlurhashBackfill(1200);
}

async function runBlurhashBackfillTick() {
  if (blurhashBackfillInFlight || !currentLibraryRoot) return;
  blurhashBackfillInFlight = true;
  try {
    const result = await invoke('backfill_missing_blurhashes', { limit: BLURHASH_BACKFILL_BATCH });
    const remaining = Number(result?.remaining ?? 0);
    if (isDev && Number(result?.processed ?? 0) > 0) {
      console.info('[blurhash] backfill batch', result);
    }
    scheduleBlurhashBackfill(remaining > 0 ? BLURHASH_BACKFILL_ACTIVE_DELAY_MS : BLURHASH_BACKFILL_IDLE_DELAY_MS);
  } catch (error) {
    if (isDev) console.warn('[blurhash] backfill failed', error);
    scheduleBlurhashBackfill(BLURHASH_BACKFILL_IDLE_DELAY_MS);
  } finally {
    blurhashBackfillInFlight = false;
  }
}

async function ensureThumbBefore404(hash) {
  const existing = thumbEnsureInFlight.get(hash);
  if (existing) {
    await existing;
    return;
  }
  const task = (async () => {
    try {
      await invoke('ensure_thumbnail', { hash });
    } catch {
      // Ignore ensure errors here; caller still handles final 404 if file is missing.
    }
  })().finally(() => {
    thumbEnsureInFlight.delete(hash);
  });
  thumbEnsureInFlight.set(hash, task);
  await task;
}

async function registerMediaProtocol() {
  protocol.handle('media', async (request) => {
    const parsed = parseMediaUrl(request.url);
    if (!parsed || !isValidHash(parsed.hash)) {
      if (isDev) console.warn('[media] Failed to parse:', request.url);
      return new Response('Invalid media URL', {
        status: 400,
        headers: { 'Content-Type': 'text/plain', 'Cache-Control': 'no-store' },
      });
    }

    // For thumbnails, resolve actual path (.jpg or .png) to support transparent thumbnails.
    let filePath = parsed.kind === 'thumb'
      ? await resolveThumbPath(parsed.hash)
      : buildBlobPath(parsed.kind, parsed.hash, parsed.ext);
    let stat;
    try {
      stat = await fs.stat(filePath);
    } catch {
      // For thumbnails, attempt on-demand regeneration before returning 404.
      if (parsed.kind === 'thumb') {
        await ensureThumbBefore404(parsed.hash);
        filePath = await resolveThumbPath(parsed.hash);
        try {
          stat = await fs.stat(filePath);
        } catch {
          // fall through to 404 below
        }
      } else if (parsed.kind === 'file') {
        // Collection covers and stale MIME metadata can request a wrong extension.
        filePath = await resolveOriginalPath(parsed.hash, parsed.ext);
        try {
          stat = await fs.stat(filePath);
        } catch {
          // fall through to 404 below
        }
      }
      if (!stat) {
        if (isDev) console.warn('[media] 404:', parsed.kind, parsed.hash.slice(0, 12), filePath);
        return new Response('Not found', {
          status: 404,
          headers: { 'Content-Type': 'text/plain', 'Cache-Control': 'no-store' },
        });
      }
    }

    // Detect MIME from actual file extension (thumbnail may be .jpg or .png).
    const actualExt = path.extname(filePath).slice(1).toLowerCase();
    const mime = parsed.kind === 'thumb' ? extToMime(actualExt || 'jpg') : extToMime(parsed.ext);
    const rangeHeader = request.headers.get('range');
    const range = parseRange(rangeHeader, stat.size);

    if (!range) {
      const stream = createReadStream(filePath);
      return new Response(Readable.toWeb(stream), {
        status: 200,
        headers: {
          'Content-Type': mime,
          'Content-Length': String(stat.size),
          'Accept-Ranges': 'bytes',
          'Cache-Control': 'public, max-age=31536000, immutable',
        },
      });
    }

    const len = range.end - range.start + 1;
    const stream = createReadStream(filePath, { start: range.start, end: range.end });
    return new Response(Readable.toWeb(stream), {
      status: 206,
      headers: {
        'Content-Type': mime,
        'Content-Length': String(len),
        'Content-Range': `bytes ${range.start}-${range.end}/${stat.size}`,
        'Accept-Ranges': 'bytes',
        'Cache-Control': 'public, max-age=31536000, immutable',
      },
    });
  });
}

/** Compute detail window size to fit image aspect ratio within available screen.
 *  Size the window to show the image at best-fit, respecting
 *  screen work area, with a margin. All sizing in the main process. */
function calcDetailWindowSize(imgW, imgH) {
  const { workArea } = screen.getPrimaryDisplay();
  const maxW = Math.round(workArea.width * 0.85);
  const maxH = Math.round(workArea.height * 0.85);

  if (!imgW || !imgH || imgW <= 0 || imgH <= 0) {
    return { width: maxW, height: maxH };
  }

  const aspect = imgW / imgH;
  let w = maxW;
  let h = Math.round(w / aspect);
  if (h > maxH) {
    h = maxH;
    w = Math.round(h * aspect);
  }
  // Enforce reasonable minimum (400x300), but keep aspect ratio
  const MIN_W = 400;
  const MIN_H = 300;
  if (w < MIN_W || h < MIN_H) {
    const scaleUp = Math.max(MIN_W / w, MIN_H / h);
    w = Math.round(w * scaleUp);
    h = Math.round(h * scaleUp);
  }
  return { width: w, height: h };
}

function createWindow(label = 'main', hash = null, width = MAIN_WINDOW_DEFAULT_WIDTH, height = MAIN_WINDOW_DEFAULT_HEIGHT) {
  const isSettings = label === 'settings';
  const isSubscriptions = label === 'subscriptions';
  const isDetail = hash != null && !isSettings && !isSubscriptions;
  const isMain = !isSettings && !isSubscriptions && !isDetail;
  const isMac = process.platform === 'darwin';

  const savedMainState = isMain ? getSavedMainWindowState() : null;
  const initialWidth = savedMainState?.width ?? width;
  const initialHeight = savedMainState?.height ?? height;
  const winOpts = {
    width: initialWidth,
    height: initialHeight,
    ...(isSettings
      ? {
          minWidth: 900,
          minHeight: 650,
          resizable: false,
          maximizable: false,
          fullscreenable: false,
          frame: false,
          transparent: false,
          backgroundColor: '#1e1e22',
        }
      : isSubscriptions
        ? {
            minWidth: 860,
            minHeight: 700,
            maxWidth: 860,
            maxHeight: 700,
            resizable: false,
            maximizable: false,
            fullscreenable: false,
            frame: false,
            transparent: false,
            backgroundColor: '#1e1e22',
          }
      : isDetail
        ? {
            frame: false,
            transparent: false,
            backgroundColor: '#1a1a1a',
          }
        : {
            ...(isMac
              ? {
                  frame: true,
                  titleBarStyle: 'hiddenInset',
                  transparent: false,
                  backgroundColor: '#0f1115',
                }
              : {
                  frame: false,
                  transparent: false,
                  backgroundColor: '#0f1115',
                }),
          }),
    show: false,
    ...(isMac && { roundedCorners: true }),
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
    },
  };
  if (isMain && savedMainState?.x != null && savedMainState?.y != null) {
    winOpts.x = savedMainState.x;
    winOpts.y = savedMainState.y;
  }

  const win = new BrowserWindow(winOpts);

  if (isDetail) {
    win.center();
  }

  windowsByLabel.set(label, win);

  const forcedShowTimer = setTimeout(() => {
    if (!win.isDestroyed() && !win.isVisible()) {
      console.warn(`[main] window '${label}' forcing show fallback (ready-to-show timeout)`);
      try {
        win.show();
      } catch (err) {
        console.error('[main] force-show failed:', err);
      }
    }
  }, 2500);

  win.once('ready-to-show', () => {
    clearTimeout(forcedShowTimer);
    try {
      if (isMain && savedMainState?.maximized) {
        win.maximize();
      }
      win.show();
      if (isMain) win.focus();
    } catch (err) {
      console.error('[main] failed to show window:', err);
    }
  });

  win.webContents.on('did-finish-load', () => {
    if (isDev) {
      console.info(`[main] window '${label}' did-finish-load`);
    }
  });
  win.webContents.on('did-fail-load', (_event, code, desc, url) => {
    console.error(`[main] window '${label}' did-fail-load`, { code, desc, url });
  });
  win.webContents.on('render-process-gone', (_event, details) => {
    console.error(`[main] window '${label}' render-process-gone`, details);
  });

  win.on('closed', () => {
    clearTimeout(forcedShowTimer);
    windowsByLabel.delete(label);
  });

  const persistMainBoundsTimer = { value: null };
  win.on('resize', () => {
    const [w, h] = win.getSize();
    win.webContents.send('picto:window-resized', { width: w, height: h });
    if (isMain) queueSaveMainWindowState(win, persistMainBoundsTimer);
  });

  win.on('move', () => {
    win.webContents.send('picto:window-moved');
    if (isMain) queueSaveMainWindowState(win, persistMainBoundsTimer);
  });

  if (isMain) {
    win.on('maximize', () => queueSaveMainWindowState(win, persistMainBoundsTimer));
    win.on('unmaximize', () => queueSaveMainWindowState(win, persistMainBoundsTimer));
    win.on('close', () => {
      queueSaveMainWindowState(win, persistMainBoundsTimer);
      if (persistMainBoundsTimer.value != null) {
        clearTimeout(persistMainBoundsTimer.value);
        persistMainBoundsTimer.value = null;
      }
      // Force a final immediate save on close.
      if (!win.isDestroyed()) {
        const normalBounds = win.getNormalBounds();
        const cfg = getCachedConfig();
        cfg.windowState = {
          ...(cfg.windowState ?? {}),
          main: {
            x: normalBounds.x,
            y: normalBounds.y,
            width: normalBounds.width,
            height: normalBounds.height,
            maximized: win.isMaximized(),
          },
        };
        void saveGlobalConfig(cfg).catch((err) => {
          if (isDev) console.warn('[main] failed to save final window state', err);
        });
      }
    });
  }

  const page = label === 'settings'
    ? 'settings'
    : label === 'subscriptions'
      ? 'subscriptions'
      : hash
        ? 'detail'
        : 'main';
  if (isDev) {
    const url = page === 'settings'
      ? `${DEV_URL}/settings.html`
      : page === 'subscriptions'
        ? `${DEV_URL}/subscriptions.html`
      : page === 'detail'
        ? `${DEV_URL}/detail.html?hash=${encodeURIComponent(hash)}`
        : DEV_URL;
    void win.loadURL(url).catch((err) => {
      console.error(`[main] window '${label}' loadURL failed`, err);
    });
    win.webContents.openDevTools({ mode: 'detach' });
  } else {
    const htmlMap = {
      settings: 'settings.html',
      subscriptions: 'subscriptions.html',
      detail: 'detail.html',
      main: 'index.html',
    };
    void win.loadFile(path.join(__dirname, '..', 'dist', htmlMap[page]), {
      query: hash ? { hash } : undefined,
    }).catch((err) => {
      console.error(`[main] window '${label}' loadFile failed`, err);
    });
  }

  return win;
}

function sendToFocusedWindow(channel, payload = null) {
  const win = BrowserWindow.getFocusedWindow() || BrowserWindow.getAllWindows()[0];
  if (win && !win.isDestroyed()) win.webContents.send(channel, payload);
}

function sendToMainWindow(channel, payload = null) {
  const mainWin = windowsByLabel.get('main');
  if (mainWin && !mainWin.isDestroyed()) {
    mainWin.webContents.send(channel, payload);
    return;
  }
  sendToFocusedWindow(channel, payload);
}

function sendToAllWindows(channel, payload = null) {
  for (const win of BrowserWindow.getAllWindows()) {
    if (!win.isDestroyed()) win.webContents.send(channel, payload);
  }
}

function openSettingsWindow() {
  const label = 'settings';
  const existing = windowsByLabel.get(label);
  if (existing && !existing.isDestroyed()) {
    existing.focus();
    return;
  }
  createWindow(label, null, 900, 650);
}

function openSubscriptionsWindow() {
  const label = 'subscriptions';
  const existing = windowsByLabel.get(label);
  if (existing && !existing.isDestroyed()) {
    existing.focus();
    return;
  }
  createWindow(label, null, 860, 700);
}

function openLibraryManager() {
  const label = 'library-manager';
  const existing = windowsByLabel.get(label);
  if (existing && !existing.isDestroyed()) {
    existing.focus();
    return;
  }
  const mainWin = windowsByLabel.get('main');
  const win = new BrowserWindow({
    width: 700,
    height: 550,
    minWidth: 600,
    minHeight: 400,
    resizable: true,
    maximizable: false,
    fullscreenable: false,
    frame: false,
    transparent: false,
    backgroundColor: '#1e1e22',
    ...(mainWin && !mainWin.isDestroyed() ? { parent: mainWin } : {}),
    show: true,
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
    },
  });

  windowsByLabel.set(label, win);
  win.on('closed', () => windowsByLabel.delete(label));

  if (isDev) {
    void win.loadURL(`${DEV_URL}/library-manager.html`);
    win.webContents.openDevTools({ mode: 'detach' });
  } else {
    void win.loadFile(path.join(__dirname, '..', 'dist', 'library-manager.html'));
  }
}

/** Check if a library path looks valid (has db/library.sqlite). */
async function isValidLibrary(libraryPath) {
  try {
    await fs.access(path.join(libraryPath, 'db', 'library.sqlite'));
    return true;
  } catch {
    // Also accept directories that don't have a DB yet (new or legacy libraries)
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

/** Show a dialog for a missing library. Returns the action taken. */
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

/** Open a library and show the main window. */
async function openLibraryAndShow(libraryPath) {
  currentLibraryRoot = libraryPath;
  await openLibrary(libraryPath);
  startBlurhashBackfill();
  await addLibraryToHistory(libraryPath);
  buildAppMenu();

  const mainWin = windowsByLabel.get('main');
  if (mainWin && !mainWin.isDestroyed()) {
    mainWin.webContents.send('library-switched', { path: libraryPath });
  } else {
    createWindow('main');
  }
}

/** Switch to a different library. */
async function switchLibrary(newPath) {
  sendToAllWindows('library-switching', { path: newPath });

  for (const [label, win] of windowsByLabel) {
    if (label !== 'main' && !win.isDestroyed()) {
      win.close();
    }
  }

  stopBlurhashBackfill();
  await closeLibrary();

  currentLibraryRoot = newPath;
  await openLibrary(newPath);
  startBlurhashBackfill();
  await addLibraryToHistory(newPath);
  buildAppMenu();

  sendToAllWindows('library-switched', { path: newPath });
}

function buildAppMenu() {
  const isMac = process.platform === 'darwin';
  const config = getCachedConfig();

  const pinned = config.pinnedLibraries || [];
  const history = config.libraryHistory || [];
  const recentItems = [];

  const pinnedInHistory = history.filter((p) => pinned.includes(p));
  for (const libPath of pinnedInHistory) {
    const name = libraryDisplayName(libPath);
    const isCurrent = libPath === currentLibraryRoot;
    recentItems.push({
      label: `\u{1F4CC} ${name}`,
      type: isCurrent ? 'checkbox' : 'normal',
      checked: isCurrent,
      click: () => {
        if (libPath !== currentLibraryRoot) switchLibrary(libPath);
      },
    });
  }

  const unpinned = history.filter((p) => !pinned.includes(p));
  if (pinnedInHistory.length > 0 && unpinned.length > 0) {
    recentItems.push({ type: 'separator' });
  }

  for (const libPath of unpinned) {
    const name = libraryDisplayName(libPath);
    const isCurrent = libPath === currentLibraryRoot;
    recentItems.push({
      label: name,
      type: isCurrent ? 'checkbox' : 'normal',
      checked: isCurrent,
      click: () => {
        if (libPath !== currentLibraryRoot) switchLibrary(libPath);
      },
    });
  }

  if (recentItems.length > 0) {
    recentItems.push({ type: 'separator' });
    recentItems.push({
      label: 'Clear History',
      click: async () => {
        const config = getCachedConfig();
          config.libraryHistory = currentLibraryRoot ? [currentLibraryRoot] : [];
        config.pinnedLibraries = config.pinnedLibraries.filter((p) => p === currentLibraryRoot);
        await saveGlobalConfig(config);
        buildAppMenu();
      },
    });
  }

  const template = [
    ...(isMac
      ? [{
          label: app.name,
          submenu: [
            { role: 'about' },
            { type: 'separator' },
            {
              label: 'Preferences…',
              accelerator: 'CmdOrCtrl+,',
              click: () => openSettingsWindow(),
            },
            { type: 'separator' },
            { role: 'services' },
            { type: 'separator' },
            { role: 'hide' },
            { role: 'hideOthers' },
            { role: 'unhide' },
            { type: 'separator' },
            { role: 'quit' },
          ],
        }]
      : []),

    {
      label: 'File',
      submenu: [
        {
          label: 'Library Manager…',
          accelerator: 'CmdOrCtrl+L',
          click: () => openLibraryManager(),
        },
        { type: 'separator' },
        {
          label: 'New Library…',
          accelerator: 'CmdOrCtrl+N',
          click: () => sendToFocusedWindow('menu:create-library'),
        },
        {
          label: 'Open Library…',
          accelerator: 'CmdOrCtrl+O',
          click: async () => {
            const result = await dialog.showOpenDialog({
              title: 'Open Library',
              properties: ['openDirectory'],
              message: 'Select a .library folder',
            });
            if (!result.canceled && result.filePaths.length > 0) {
              const picked = result.filePaths[0];
              if (!path.basename(picked).endsWith('.library')) {
                await dialog.showMessageBox({
                  type: 'error',
                  title: 'Invalid Library',
                  message: 'The selected folder is not a library.',
                  detail: 'Please select a folder ending in .library',
                });
                return;
              }
              await switchLibrary(picked);
            }
          },
        },
        { type: 'separator' },
        ...(recentItems.length > 0
          ? [{ label: 'Recent Libraries', submenu: recentItems }, { type: 'separator' }]
          : []),
        {
          label: 'Import Files…',
          accelerator: 'CmdOrCtrl+I',
          click: () => sendToFocusedWindow('menu:import-files'),
        },
        { type: 'separator' },
        isMac ? { role: 'close' } : { role: 'quit' },
      ],
    },

    {
      label: 'Edit',
      submenu: [
        {
          label: 'Undo',
          accelerator: 'CmdOrCtrl+Z',
          click: () => sendToMainWindow('menu:undo'),
        },
        {
          label: 'Redo',
          accelerator: isMac ? 'Shift+Cmd+Z' : 'Ctrl+Y',
          click: () => sendToMainWindow('menu:redo'),
        },
        { type: 'separator' },
        { role: 'cut' },
        { role: 'copy' },
        { role: 'paste' },
        { role: 'selectAll' },
        ...(!isMac ? [
          { type: 'separator' },
          {
            label: 'Preferences…',
            accelerator: 'CmdOrCtrl+,',
            click: () => openSettingsWindow(),
          },
        ] : []),
      ],
    },

    {
      label: 'View',
      submenu: [
        {
          label: 'All Images',
          accelerator: 'CmdOrCtrl+1',
          click: () => sendToFocusedWindow('menu:navigate', 'images'),
        },
        {
          label: 'Inbox',
          accelerator: 'CmdOrCtrl+2',
          click: () => sendToFocusedWindow('menu:navigate', 'review'),
        },
        {
          label: 'Untagged',
          accelerator: 'CmdOrCtrl+3',
          click: () => sendToFocusedWindow('menu:navigate', 'untagged'),
        },
        {
          label: 'Recently Viewed',
          accelerator: 'CmdOrCtrl+4',
          click: () => sendToFocusedWindow('menu:navigate', 'recently_viewed'),
        },
        {
          label: 'Trash',
          accelerator: 'CmdOrCtrl+5',
          click: () => sendToFocusedWindow('menu:navigate', 'trash'),
        },
        { type: 'separator' },
        ...(isDev ? [
          { role: 'reload' },
          { role: 'forceReload' },
          { role: 'toggleDevTools' },
          { type: 'separator' },
        ] : []),
        { role: 'togglefullscreen' },
      ],
    },

    {
      label: 'Window',
      submenu: [
        { role: 'minimize' },
        { role: 'zoom' },
        ...(isMac ? [{ type: 'separator' }, { role: 'front' }] : [{ role: 'close' }]),
      ],
    },

    {
      label: 'Help',
      submenu: [
        {
          label: 'About Picto',
          click: () => openSettingsWindow(),
        },
      ],
    },
  ];

  const menu = Menu.buildFromTemplate(template);
  Menu.setApplicationMenu(menu);
}

function setupIpc() {
  ipcMain.handle('picto:invoke', async (_event, payload) => {
    const { command, args } = payload || {};
    if (!command || typeof command !== 'string') {
      throw new Error('Invalid invoke payload');
    }

    if (command === 'open_settings_window') {
      openSettingsWindow();
      return null;
    }

    if (command === 'open_subscriptions_window') {
      openSubscriptionsWindow();
      return null;
    }

    if (command === 'open_library_manager') {
      openLibraryManager();
      return null;
    }

    if (command === 'open_in_new_window') {
      const hash = args?.hash;
      if (!isValidHash(hash)) throw new Error('Invalid hash for detail window');
      const label = `detail-${hash.slice(0, 12)}`;
      const existing = windowsByLabel.get(label);
      if (existing && !existing.isDestroyed()) {
        existing.focus();
        return null;
      }
      const { width, height } = calcDetailWindowSize(args?.width, args?.height);
      createWindow(label, hash, width, height);
      return null;
    }

    return invoke(command, args || {});
  });

  ipcMain.handle('picto:event:emit', (event, { name, payload, target }) => {
    if (!name || typeof name !== 'string') return null;
    if (target) {
      const win = windowsByLabel.get(target);
      if (win && !win.isDestroyed()) win.webContents.send(name, payload);
      return null;
    }
    for (const win of BrowserWindow.getAllWindows()) {
      if (win.webContents.id !== event.sender.id) {
        win.webContents.send(name, payload);
      }
    }
    return null;
  });

  ipcMain.handle('picto:window', (event, { method, payload }) => {
    const win = BrowserWindow.fromWebContents(event.sender);
    if (!win) throw new Error('No window context');
    switch (method) {
      case 'show': win.show(); return null;
      case 'setTheme': {
        const theme = payload?.theme;
        if (theme === 'dark' || theme === 'light' || theme === 'system') {
          nativeTheme.themeSource = theme;
        }
        return null;
      }
      case 'startDragging': {
        // Electron's native window dragging via webContents
        // The renderer should use -webkit-app-region: drag CSS instead,
        // but we support the IPC fallback
        return null;
      }
      case 'minimize': win.minimize(); return null;
      case 'toggleMaximize': win.isMaximized() ? win.unmaximize() : win.maximize(); return null;
      case 'setSize': {
        const w = Number(payload?.width);
        const h = Number(payload?.height);
        if (!Number.isFinite(w) || !Number.isFinite(h)) throw new Error('Invalid size');
        win.setSize(Math.round(w), Math.round(h));
        return null;
      }
      case 'setAlwaysOnTop':
        win.setAlwaysOnTop(Boolean(payload?.value));
        return null;
      case 'close':
        win.close();
        return null;
      case 'setFocus':
        win.focus();
        return null;
      default:
        throw new Error(`Unknown window method: ${method}`);
    }
  });

  ipcMain.handle('picto:dialog:open', async (_event, options = {}) => {
    const result = await dialog.showOpenDialog({
      properties: ['openFile', 'multiSelections'],
      ...options,
    });
    if (result.canceled) return null;
    if (result.filePaths.length === 0) return null;
    return result.filePaths.length === 1 ? result.filePaths[0] : result.filePaths;
  });

  ipcMain.handle('picto:dialog:save', async (_event, options = {}) => {
    const result = await dialog.showSaveDialog(options);
    return result.canceled ? null : result.filePath ?? null;
  });

  ipcMain.handle('picto:clipboard:writeText', (_event, { text }) => {
    clipboard.writeText(String(text ?? ''));
    return null;
  });

  ipcMain.handle('picto:clipboard:copyFile', async (_event, { filePath }) => {
    // macOS: write file URL to pasteboard so Finder can paste
    // Windows/Linux: write file path as text (best effort)
    if (process.platform === 'darwin') {
      // NSPasteboard file URL — Electron doesn't expose this directly,
      // but we can write the path as text + bookmark it via file:// URI
      clipboard.writeBookmark(path.basename(filePath), `file://${filePath}`);
    } else {
      clipboard.writeText(filePath);
    }
    return null;
  });

  ipcMain.handle('picto:clipboard:copyImage', async (_event, { filePath }) => {
    try {
      const img = nativeImage.createFromPath(filePath);
      if (img.isEmpty()) throw new Error('Failed to load image');
      clipboard.writeImage(img);
      return null;
    } catch (err) {
      throw new Error(`Failed to copy image: ${err.message}`);
    }
  });

  ipcMain.handle('picto:reverseImageSearch', async (_event, { filePath, engine }) => {
    // __waitFor helper for pre-setup scripts (click buttons, reveal hidden inputs)
    const waitForHelper = `
      function __waitFor(sel, timeout = 10000) {
        return new Promise((resolve, reject) => {
          if (document.querySelector(sel)) return resolve(document.querySelector(sel));
          const obs = new MutationObserver(() => {
            const el = document.querySelector(sel);
            if (el) { obs.disconnect(); resolve(el); }
          });
          obs.observe(document.documentElement, { childList: true, subtree: true, attributes: true });
          setTimeout(() => { obs.disconnect(); reject(new Error('Timeout waiting for: ' + sel)); }, timeout);
        });
      }
    `;

    // Per-engine configs: preSetup clicks buttons/waits for file input to appear,
    // then CDP injects the file (like Puppeteer), then postSetup submits if needed.
    const engineConfigs = {
      tineye: {
        url: 'https://tineye.com/',
        preSetup: `(async () => { ${waitForHelper} await __waitFor('#upload-box'); })()`,
        fileInputSelector: "input[type='file']",
        postSetup: null,
        isResultUrl: (href) => /tineye\.com\/search\//.test(href),
      },
      saucenao: {
        url: 'https://saucenao.com/',
        preSetup: `(async () => { ${waitForHelper} await __waitFor('#searchForm'); })()`,
        fileInputSelector: "input[type='file']",
        postSetup: `document.querySelector('#searchForm')?.submit()`,
        isResultUrl: (href) => href.includes('saucenao.com/search.php'),
      },
      yandex: {
        url: 'https://yandex.com/images/',
        preSetup: `(async () => {
          ${waitForHelper}
          // Click the camera/CBIR button to trigger SmartCamera module
          await __waitFor('.input__cbir-button, [data-bem*="cbir"], .HeaderDesktopForm-CbirButton, button[aria-label*="image"]');
          const btn = document.querySelector('.input__cbir-button')
            || document.querySelector('[data-bem*="cbir"]')
            || document.querySelector('.HeaderDesktopForm-CbirButton')
            || document.querySelector('button[aria-label*="image"]');
          if (btn) { btn.click(); await new Promise(r => setTimeout(r, 1000)); }
          await __waitFor("input[type='file']", 8000);
        })()`,
        fileInputSelector: "input[type='file']",
        postSetup: null,
        isResultUrl: (href) => /yandex\.(com|eu|ru)\/images\/search/.test(href),
      },
      sogou: {
        url: 'https://pic.sogou.com/',
        preSetup: `(async () => {
          ${waitForHelper}
          await __waitFor('#cameraIco, .camera-ico, [class*="camera"]');
          const btn = document.querySelector('#cameraIco') || document.querySelector('.camera-ico') || document.querySelector('[class*="camera"]');
          if (btn) { btn.click(); await new Promise(r => setTimeout(r, 800)); }
          await __waitFor("input[type='file']", 5000);
        })()`,
        fileInputSelector: "input[type='file']",
        postSetup: null,
        isResultUrl: (href) => href.includes('/ris'),
      },
      bing: {
        url: 'https://www.bing.com/images',
        preSetup: `(async () => {
          ${waitForHelper}
          await __waitFor('#sb_sbip, #sb_sbi, [id*="sbi"], input[type="file"]');
          const btn = document.querySelector('#sb_sbip') || document.querySelector('#sb_sbi') || document.querySelector('[id*="sbi"]');
          if (btn) { btn.click(); await new Promise(r => setTimeout(r, 1000)); }
          await __waitFor("input[type='file']", 8000);
        })()`,
        fileInputSelector: "input[type='file']",
        postSetup: null,
        isResultUrl: (href) => href.includes('bing.com/images/search'),
      },
    };

    const cfg = engineConfigs[engine];
    if (!cfg) throw new Error(`Unknown search engine: ${engine}`);

    const searchWin = new BrowserWindow({
      show: false,
      width: 1100,
      height: 800,
      webPreferences: { contextIsolation: true, nodeIntegration: false },
    });
    searchWin.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));

    try {
      console.log(`[reverse-search] ${engine}: loading ${cfg.url}`);
      await Promise.race([
        searchWin.webContents.loadURL(cfg.url),
        new Promise((_, rej) => setTimeout(() => rej(new Error('Page load timed out')), 15_000)),
      ]);

      console.log(`[reverse-search] ${engine}: running pre-setup`);
      await Promise.race([
        searchWin.webContents.executeJavaScript(cfg.preSetup, true),
        new Promise((_, rej) => setTimeout(() => rej(new Error('Pre-setup timed out')), 15_000)),
      ]);

      console.log(`[reverse-search] ${engine}: injecting file via CDP`);
      searchWin.webContents.debugger.attach('1.3');
      try {
        const { root } = await searchWin.webContents.debugger.sendCommand('DOM.getDocument');
        const { nodeId } = await searchWin.webContents.debugger.sendCommand('DOM.querySelector', {
          nodeId: root.nodeId,
          selector: cfg.fileInputSelector,
        });
        if (!nodeId) throw new Error(`File input not found: ${cfg.fileInputSelector}`);
        await searchWin.webContents.debugger.sendCommand('DOM.setFileInputFiles', {
          nodeId,
          files: [filePath],
        });
        console.log(`[reverse-search] ${engine}: file injected`);
      } finally {
        try { searchWin.webContents.debugger.detach(); } catch {}
      }

      if (cfg.postSetup) {
        await searchWin.webContents.executeJavaScript(cfg.postSetup, true);
      }

      console.log(`[reverse-search] ${engine}: polling for result URL`);
      const resultUrl = await new Promise((resolve, reject) => {
        let attempts = 0;
        const interval = setInterval(async () => {
          attempts++;
          if (searchWin.isDestroyed()) {
            clearInterval(interval);
            reject(new Error('Search window was closed'));
            return;
          }
          try {
            const href = await searchWin.webContents.executeJavaScript('location.href', true);
            if (cfg.isResultUrl(href)) {
              clearInterval(interval);
              resolve(href);
            }
            if (attempts % 50 === 0) console.log(`[reverse-search] ${engine}: polling (${attempts}), URL: ${href}`);
            if (attempts > 300) {
              clearInterval(interval);
              reject(new Error(`${engine}: timed out. Last URL: ${href}`));
            }
          } catch { /* transient nav errors */ }
        }, 100);
      });

      console.log(`[reverse-search] ${engine}: opening in default browser: ${resultUrl}`);
      await shell.openExternal(resultUrl);
      return resultUrl;
    } finally {
      if (!searchWin.isDestroyed()) searchWin.destroy();
    }
  });

  ipcMain.handle('picto:drag:start', async (event, { hashes, iconDataUrl }) => {
    if (!hashes?.length) return null;

    // Only resolve the first file path — internal operations (reorder, sidebar
    // folder drops) use in-renderer imageDrag state and don't need OS file paths.
    // Resolving only one path avoids a potentially slow bulk resolution (1000+ hashes).
    let filePath;
    try {
      filePath = await invoke('resolve_file_path', { hash: hashes[0] });
    } catch { return null; }
    if (!filePath) return null;

    // Drag icon: prefer renderer-provided icon (has count badge + proper
    // positioning); fall back to the file's thumbnail.
    let icon;
    if (iconDataUrl) {
      try {
        icon = nativeImage.createFromDataURL(iconDataUrl);
        if (icon.isEmpty()) icon = null;
      } catch { icon = null; }
    }
    if (!icon) {
      const thumbPath = buildBlobPath('thumb', hashes[0], 'jpg');
      try {
        icon = nativeImage.createFromPath(thumbPath);
        if (icon.isEmpty()) icon = null;
      } catch { icon = null; }
      if (icon) icon = icon.resize({ width: 64 });
    }

    // Pass a single file to startDrag to prevent the OS from generating an
    // enormous stacked drag visualization when hundreds of files are selected.
    // Internal drops never use the OS file list — they read from imageDrag state.
    event.sender.startDrag({
      files: [filePath],
      icon: icon || nativeImage.createEmpty(),
    });

    return { ok: true };
  });

  ipcMain.handle('picto:popup-menu', (event) => {
    const win = BrowserWindow.fromWebContents(event.sender);
    if (win) Menu.getApplicationMenu()?.popup({ window: win });
  });

  ipcMain.handle('picto:monitor:current', () => {
    const point = screen.getCursorScreenPoint();
    const display = screen.getDisplayNearestPoint(point);
    return {
      scaleFactor: display.scaleFactor,
      size: { width: display.size.width, height: display.size.height },
    };
  });

  ipcMain.handle('picto:library:create', async (_event, { name, savePath }) => {
    const libraryPath = path.join(savePath, `${name}.library`);
    await fs.mkdir(path.join(libraryPath, 'db'), { recursive: true });
    await fs.mkdir(path.join(libraryPath, 'blobs'), { recursive: true });
    await fs.mkdir(path.join(libraryPath, 'plugins', 'sites'), { recursive: true });
    await switchLibrary(libraryPath);
    return libraryPath;
  });

  ipcMain.handle('picto:library:open', async () => {
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
  });

  ipcMain.handle('picto:library:switch', async (_event, { path: libPath }) => {
    if (!path.basename(libPath).endsWith('.library')) {
      throw new Error('Not a valid .library folder');
    }
    await switchLibrary(libPath);
  });

  ipcMain.handle('picto:library:remove', async (_event, { path: libPath }) => {
    await removeLibraryFromHistory(libPath);
    buildAppMenu();
  });

  ipcMain.handle('picto:library:delete', async (_event, { path: libPath }) => {
    if (libPath === currentLibraryRoot) {
      throw new Error('Cannot delete the currently open library. Switch to a different library first.');
    }
    const name = libraryDisplayName(libPath);
    const result = await dialog.showMessageBox({
      type: 'warning',
      title: 'Delete Library',
      message: `Delete library "${name}"?`,
      detail: `This will permanently delete all files in:\n${libPath}\n\nThis cannot be undone.`,
      buttons: ['Delete', 'Cancel'],
      defaultId: 1,
      cancelId: 1,
      destructiveId: 0,
    });
    if (result.response !== 0) return { deleted: false };
    await removeLibraryFromHistory(libPath);
    await fs.rm(libPath, { recursive: true, force: true });
    buildAppMenu();
    return { deleted: true };
  });

  ipcMain.handle('picto:library:togglePin', async (_event, { path: libPath }) => {
    await togglePinned(libPath);
    buildAppMenu();
  });

  ipcMain.handle('picto:library:rename', async (_event, { path: libPath, newName }) => {
    // Validate name
    if (!newName || typeof newName !== 'string' || !newName.trim()) {
      throw new Error('Library name cannot be empty');
    }
    const cleanName = newName.trim();
    if (/[/\\]/.test(cleanName)) {
      throw new Error('Library name cannot contain slashes');
    }

    const parentDir = path.dirname(libPath);
    const newPath = path.join(parentDir, `${cleanName}.library`);

    // Check target doesn't already exist
    try {
      await fs.access(newPath);
      throw new Error(`A library named "${cleanName}" already exists at that location`);
    } catch (err) {
      if (err.code !== 'ENOENT') throw err;
    }

    if (libPath === currentLibraryRoot) {
      // Renaming the current library — need to close and reopen
      sendToAllWindows('library-switching', { path: newPath });
      stopBlurhashBackfill();
      await closeLibrary();
      try {
        await fs.rename(libPath, newPath);
      } catch (err) {
        // Rollback: reopen the old library
        await openLibrary(libPath);
        startBlurhashBackfill();
        sendToAllWindows('library-switched', { path: libPath });
        throw new Error(`Failed to rename library: ${err.message}`);
      }
      await updateLibraryPath(libPath, newPath);
      currentLibraryRoot = newPath;
      await openLibrary(newPath);
      startBlurhashBackfill();
      sendToAllWindows('library-switched', { path: newPath });
      buildAppMenu();
    } else {
      // Renaming a non-current library
      await fs.rename(libPath, newPath);
      await updateLibraryPath(libPath, newPath);
      buildAppMenu();
    }

    return { newPath };
  });

  ipcMain.handle('picto:library:relocate', async (_event, { oldPath }) => {
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

    // Don't move onto itself
    if (newPath === oldPath) {
      return { action: 'cancelled' };
    }

    // Check destination doesn't already exist
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
      // Move the library folder to the new location
      if (oldPath === currentLibraryRoot) {
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
        currentLibraryRoot = newPath;
        await openLibrary(newPath);
        startBlurhashBackfill();
        sendToAllWindows('library-switched', { path: newPath });
      } else {
        await fs.rename(oldPath, newPath);
        await updateLibraryPath(oldPath, newPath);
      }
    } else {
      // Library is missing — just update the config to point to new location
      await updateLibraryPath(oldPath, newPath);
    }

    buildAppMenu();
    return { action: 'relocated', newPath };
  });

  ipcMain.handle('picto:library:getConfig', async () => {
    const config = getCachedConfig();
    const existsMap = {};
    await Promise.all(
      (config.libraryHistory ?? []).map(async (p) => {
        try {
          await fs.access(p);
          existsMap[p] = true;
        } catch {
          existsMap[p] = false;
        }
      }),
    );
    return {
      ...config,
      currentPath: currentLibraryRoot,
      existsMap,
    };
  });
}

process.on('uncaughtException', (err) => {
  console.error('[main] Uncaught exception:', err);
});
process.on('unhandledRejection', (reason) => {
  console.error('[main] Unhandled promise rejection:', reason);
});

app.whenReady().then(async () => {
  console.info('[main] app.whenReady begin');
  await registerMediaProtocol();
  console.info('[main] media protocol registered');
  setupIpc();
  console.info('[main] IPC handlers registered');

  // Register event bridge before initializing core so events are wired
  onNativeEvent((name, payload) => {
    if (!name || typeof name !== 'string') return;

    // Handle detail window open requests from core
    if (name === 'open-detail-window') {
      try {
        const data = typeof payload === 'string' ? JSON.parse(payload) : payload;
        const hash = data?.hash;
        if (isValidHash(hash)) {
          const label = `detail-${hash.slice(0, 12)}`;
          const existing = windowsByLabel.get(label);
          if (existing && !existing.isDestroyed()) {
            existing.focus();
          } else {
            const size = calcDetailWindowSize(data?.width ?? 0, data?.height ?? 0);
            createWindow(label, hash, size.width, size.height);
          }
        }
      } catch { /* ignore parse errors */ }
      return;
    }

    for (const win of BrowserWindow.getAllWindows()) {
      if (!win.isDestroyed()) {
        win.webContents.send(name, payload);
      }
    }
  });

  // Load global config and determine which library to open
  const config = await loadGlobalConfig();
  console.info('[main] global config loaded');

  // Determine library to open (priority: env var > last library > first valid)
  let libraryToOpen = null;

  if (process.env.PICTO_LIBRARY_ROOT) {
    libraryToOpen = process.env.PICTO_LIBRARY_ROOT;
  } else if (config.lastLibrary && await isValidLibrary(config.lastLibrary)) {
    libraryToOpen = config.lastLibrary;
  } else {
    // Try history entries
    for (const libPath of config.libraryHistory) {
      if (await isValidLibrary(libPath)) {
        libraryToOpen = libPath;
        break;
      }
    }
  }

  // Handle missing library
  if (!libraryToOpen && config.lastLibrary) {
    const result = await handleMissingLibrary(config.lastLibrary);
    if (result.action === 'relocated') {
      libraryToOpen = result.newPath;
    }
  }

  if (libraryToOpen) {
    // Open the selected library.
    currentLibraryRoot = libraryToOpen;
    console.info('[main] initializing library', { libraryToOpen });
    await initialize(libraryToOpen);
    console.info('[main] library initialized in native core');
    await addLibraryToHistory(libraryToOpen);
    console.info('[main] library history updated');
  } else {
    // First launch / no valid library: start app without creating any default library.
    currentLibraryRoot = null;
    console.info('[main] no initial library selected; starting without an open library');
  }

  buildAppMenu();
  console.info('[main] app menu built');

  console.info('[main] creating main window');
  createWindow('main');
  console.info('[main] main window creation requested');

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow('main');
  });
}).catch((err) => {
  console.error('[main] app.whenReady failed:', err);
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit();
});
