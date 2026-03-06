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

export function calcDetailWindowSize(screen, imgW, imgH) {
  const { workArea } = screen.getPrimaryDisplay();
  const maxW = Math.round(workArea.width * 0.85);
  const maxH = Math.round(workArea.height * 0.85);

  if (!imgW || !imgH || imgW <= 0 || imgH <= 0) {
    return { width: maxW, height: maxH };
  }

  const aspect = imgW / imgH;
  let width = maxW;
  let height = Math.round(width / aspect);
  if (height > maxH) {
    height = maxH;
    width = Math.round(height * aspect);
  }
  const minWidth = 400;
  const minHeight = 300;
  if (width < minWidth || height < minHeight) {
    const scaleUp = Math.max(minWidth / width, minHeight / height);
    width = Math.round(width * scaleUp);
    height = Math.round(height * scaleUp);
  }
  return { width, height };
}

export function createWindowManager({
  BrowserWindow,
  screen,
  path,
  __dirname,
  DEV_URL,
  isDev,
  getCachedConfig,
  saveGlobalConfig,
}) {
  const windowsByLabel = new Map();

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
    const intersectsAnyDisplay = displays.some((display) => rectsIntersect(rect, display.workArea));
    if (!intersectsAnyDisplay) {
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
        if (isDev) console.warn('[main] failed to save window state', err);
      });
    }, WINDOW_STATE_SAVE_DEBOUNCE_MS);
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

  function getWindow(label) {
    return windowsByLabel.get(label);
  }

  function getAllWindows() {
    return BrowserWindow.getAllWindows();
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

  return {
    calcDetailWindowSize: (imgW, imgH) => calcDetailWindowSize(screen, imgW, imgH),
    createWindow,
    getAllWindows,
    getWindow,
    openLibraryManager,
    openSettingsWindow,
    openSubscriptionsWindow,
    sendToAllWindows,
    sendToFocusedWindow,
    sendToMainWindow,
  };
}
