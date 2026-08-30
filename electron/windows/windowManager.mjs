import fs from 'node:fs';
import { createAuthSessions } from './authSessions.mjs';
import { createRequire } from 'node:module';

const isMac = process.platform === 'darwin';
const isWin = process.platform === 'win32';
const esmRequire = createRequire(import.meta.url);

/** Map theme name to a background color for BrowserWindow creation. */
const THEME_BG_COLORS = {
  dark:        '#1a1a1e',
  blue:        '#0f1732',
  purple:      '#1e1526',
  gray:        '#323236',
  light:       '#ebedef',
  lightgray:   '#c8cacd',
  auto:        null,
  // Native transparency themes — bg is transparent
  vibrancy:    '#00000000',
  liquidglass: '#00000000',
  mica:        '#00000000',
  acrylic:     '#00000000',
};

/** Native transparency themes that need special BrowserWindow options. */
const NATIVE_THEMES = new Set(['vibrancy', 'liquidglass', 'mica', 'acrylic']);

export function resolveThemeInfo(value, shouldUseDarkColors = false) {
  let theme = typeof value === 'string' && Object.hasOwn(THEME_BG_COLORS, value)
    ? value
    : 'dark';
  if (theme === 'auto') theme = shouldUseDarkColors ? 'dark' : 'light';
  return { theme, bgColor: THEME_BG_COLORS[theme] ?? THEME_BG_COLORS.dark };
}

/** Read the app-level preference, with one legacy settings.json fallback. */
function getThemeInfo(getCachedConfig) {
  const config = getCachedConfig();
  let theme = config?.theme;
  try {
    const libraryPath = config?.lastLibrary;
    if (!theme && libraryPath) {
      const settingsPath = libraryPath + '/settings.json';
      const raw = fs.readFileSync(settingsPath, 'utf-8');
      const settings = JSON.parse(raw);
      theme = settings.colorScheme || settings.theme || 'dark';
    }
  } catch {}
  try {
    const { nativeTheme } = esmRequire('electron');
    return resolveThemeInfo(theme, nativeTheme.shouldUseDarkColors);
  } catch {
    return resolveThemeInfo(theme);
  }
}

const MAIN_WINDOW_DEFAULT_WIDTH = 1200;
const MAIN_WINDOW_DEFAULT_HEIGHT = 800;
const MAIN_WINDOW_MIN_WIDTH = 700;
const MAIN_WINDOW_MIN_HEIGHT = 500;
const WINDOW_STATE_SAVE_DEBOUNCE_MS = 180;

export function windowResizePersistenceEvent(platform = process.platform) {
  return platform === 'darwin' || platform === 'win32' ? 'resized' : 'resize';
}

// The 2x reference requires a 26px horizontal and 34px vertical rendered
// inset. Account for the native frame's 2px/4px physical offsets.
const MAC_TRAFFIC_LIGHT_POSITION = { x: 12, y: 15 };

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

export function calcDetailWindowAspectRatio(imgW, imgH) {
  const width = Number(imgW);
  const height = Number(imgH);
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
    return null;
  }
  return width / height;
}

export function createWindowManager({
  BrowserWindow,
  screen,
  path,
  invoke,
  __dirname,
  DEV_URL,
  isDev,
  getCachedConfig,
  saveGlobalConfig,
  onWindowEvent = () => {},
}) {
  const windowsByLabel = new Map();
  const shouldOpenDevTools = isDev && process.env.PICTO_OPEN_DEVTOOLS === '1';
  const authSessions = createAuthSessions({
    BrowserWindow,
    getMainWindow,
    persistCredential: (credential) => invoke('auth.credentials.set', credential),
    beginPixivOAuth: () => invoke('pixiv_oauth_start'),
    completePixivOAuth: (input) => invoke('pixiv_oauth_exchange', input),
  });
  function getMainWindow() {
    const win = windowsByLabel.get('main');
    return win && !win.isDestroyed() ? win : null;
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

  function createWindow(
    label = 'main',
    hash = null,
    width = MAIN_WINDOW_DEFAULT_WIDTH,
    height = MAIN_WINDOW_DEFAULT_HEIGHT,
    detailItemId = null,
    detailAspectRatio = null,
  ) {
    const isSettings = label === 'settings';
    const isSubscriptions = label === 'subscriptions';
    const isDetail = (hash != null || detailItemId != null) && !isSettings && !isSubscriptions;
    const isMain = !isSettings && !isSubscriptions && !isDetail;
    const savedMainState = isMain ? getSavedMainWindowState() : null;
    const initialWidth = savedMainState?.width ?? width;
    const initialHeight = savedMainState?.height ?? height;
    const { theme: currentTheme, bgColor: themeBg } = getThemeInfo(getCachedConfig);
    const isNativeTheme = NATIVE_THEMES.has(currentTheme);
    // Native themes: transparent + vibrancy on macOS for main, settings, and subscriptions windows.
    // Liquid glass falls back to vibrancy (addView blocks input — see above).
    const isSecondary = isSettings || isSubscriptions;
    const useTransparent = isNativeTheme && isMac && (isMain || isSecondary);
    const useVibrancy = isMac && (currentTheme === 'vibrancy' || currentTheme === 'liquidglass');
    const winOpts = {
      width: initialWidth,
      height: initialHeight,
      ...(isSettings
        ? {
            minWidth: 900,
            minHeight: 650,
            // Keep the settings surface on the native resize path. The old
            // frameless, fixed-size window only exposed a very narrow edge
            // hit target (and no useful corner target) on macOS.
            resizable: true,
            maximizable: false,
            fullscreenable: false,
            ...(isMac
              ? {
                  frame: true,
                  titleBarStyle: 'hiddenInset',
                }
              : { frame: false }),
            transparent: useTransparent,
            backgroundColor: useTransparent ? '#00000000' : (themeBg === '#00000000' ? '#1a1a1e' : themeBg),
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
              transparent: useTransparent,
              backgroundColor: useTransparent ? '#00000000' : (themeBg === '#00000000' ? '#1a1a1e' : themeBg),
            }
          : isDetail
            ? {
                frame: false,
                transparent: useTransparent,
                backgroundColor: themeBg,
              }
            : {
                minWidth: 1000,
                minHeight: 600,
                ...(isMac
                  ? {
                      frame: true,
                      titleBarStyle: 'hiddenInset',
                      trafficLightPosition: MAC_TRAFFIC_LIGHT_POSITION,
                      transparent: useTransparent,
                      backgroundColor: themeBg,
                    }
                  : {
                      frame: false,
                      transparent: useTransparent,
                      backgroundColor: themeBg,
                    }),
              }),
      show: false,
      ...(isMac && { roundedCorners: true }),
      // macOS vibrancy — applied at creation time (zero-frame)
      ...(useVibrancy && {
        vibrancy: 'under-window',
        visualEffectState: 'active',
      }),
      // Liquid glass on main: NO vibrancy (addView handles it)
      ...(isMain && currentTheme === 'mica' && isWin && { backgroundMaterial: 'mica' }),
      ...(isMain && currentTheme === 'acrylic' && isWin && { backgroundMaterial: 'acrylic' }),
      webPreferences: {
        preload: path.join(__dirname, 'preload.cjs'),
        contextIsolation: true,
        nodeIntegration: false,
        sandbox: true,
      },
    };
    if (isMain && savedMainState?.x != null && savedMainState?.y != null) {
      winOpts.x = savedMainState.x;
      winOpts.y = savedMainState.y;
    }

    const win = new BrowserWindow(winOpts);

    // Let the OS own live resizing so the detail surface stays exactly aligned
    // with the media without renderer-side resize work or intermediate reflow.
    if (isDetail && detailAspectRatio != null) {
      win.setAspectRatio(detailAspectRatio);
    }

    // Settings owns its close control, but retaining the hidden native frame
    // gives macOS its normal, forgiving edge and corner resize hit areas.
    if (isSettings && isMac) {
      win.setWindowButtonVisibility(false);
    }

    // macOS Liquid Glass — electron-liquid-glass addView() blocks all mouse input
    // (NSGlassEffectView intercepts hit tests, no known fix as of v1.1.1).
    // Fall back to vibrancy which gives a similar frosted effect with working input.
    const needsLiquidGlass = false; // disabled — see above
    win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));

    if (isDetail) {
      win.center();
    }

    windowsByLabel.set(label, win);

    const forcedShowTimer = setTimeout(() => {
      if (!win.isDestroyed() && !win.isVisible()) {
        console.warn(`[main] window '${label}' forcing show fallback (ready-to-show timeout)`);
        try {
          if (isDev && isMain) win.showInactive();
          else win.show();
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
        if (isDev && isMain) {
          win.showInactive();
        } else {
          win.show();
          if (isMain) win.focus();
        }
      } catch (err) {
        console.error('[main] failed to show window:', err);
      }
    });

    win.webContents.on('did-finish-load', () => {
      onWindowEvent('did-finish-load', { label });
      if (isDev) {
        console.info(`[main] window '${label}' did-finish-load`);
      }
    });
    win.webContents.on('did-fail-load', (_event, code, desc, url) => {
      onWindowEvent('did-fail-load', { label, code, desc, url });
      console.error(`[main] window '${label}' did-fail-load`, { code, desc, url });
    });
    win.webContents.on('preload-error', (_event, preloadPath, error) => {
      onWindowEvent('preload-error', {
        label,
        preloadPath,
        message: error?.message ?? String(error),
      });
      console.error(`[main] window '${label}' preload-error`, error);
    });
    win.webContents.on('render-process-gone', (_event, details) => {
      onWindowEvent('render-process-gone', { label, reason: details?.reason, exitCode: details?.exitCode });
      console.error(`[main] window '${label}' render-process-gone`, details);
    });
    win.on('error', (error) => {
      onWindowEvent('window-error', { label, message: error?.message ?? String(error) });
      console.error(`[main] window '${label}' error`, error);
    });

    win.on('closed', () => {
      clearTimeout(forcedShowTimer);
      windowsByLabel.delete(label);
      if (label === 'main') {
        void authSessions.cancelAuthSession().catch((error) => {
          console.warn('[main] failed to clean up auth session:', error);
        });
      }
    });

    const persistMainBoundsTimer = { value: null };
    if (isMain) {
      // macOS and Windows expose a settled event, so keep all JavaScript and
      // persistence work out of the native live-resize loop. Linux has no
      // `resized` event and retains the debounced continuous-event fallback.
      win.on(windowResizePersistenceEvent(), () => {
        queueSaveMainWindowState(win, persistMainBoundsTimer);
      });
    }

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
        : isDetail
          ? 'detail'
          : 'main';
    if (isDev) {
      const url = page === 'settings'
        ? `${DEV_URL}/settings.html`
        : page === 'subscriptions'
          ? `${DEV_URL}/subscriptions.html`
          : page === 'detail'
            ? hash != null
              ? `${DEV_URL}/detail.html?hash=${encodeURIComponent(hash)}`
              : `${DEV_URL}/detail.html?item_id=${encodeURIComponent(detailItemId)}`
            : DEV_URL;
      void win.loadURL(url).catch((err) => {
        console.error(`[main] window '${label}' loadURL failed`, err);
      });
      if (shouldOpenDevTools) {
        win.webContents.openDevTools({ mode: 'detach', activate: false });
      }
    } else {
      const htmlMap = {
        settings: 'settings.html',
        subscriptions: 'subscriptions.html',
        detail: 'detail.html',
        main: 'index.html',
      };
      void win.loadFile(path.join(__dirname, '..', 'dist', htmlMap[page]), {
        query: hash != null
          ? { hash }
          : detailItemId != null
            ? { item_id: String(detailItemId) }
            : undefined,
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

  async function setThemePreference(theme) {
    if (typeof theme !== 'string' || !Object.hasOwn(THEME_BG_COLORS, theme)) return;
    const config = getCachedConfig();
    if (config.theme === theme) return;
    await saveGlobalConfig({ ...config, theme });
  }

  function ownsWebContents(contents) {
    if (!contents || contents.isDestroyed?.()) return false;
    for (const win of windowsByLabel.values()) {
      if (!win.isDestroyed() && win.webContents === contents) return true;
    }
    return false;
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

  function openSettingsWindow(panel = null) {
    const label = 'settings';
    const existing = windowsByLabel.get(label);
    if (existing && !existing.isDestroyed()) {
      existing.focus();
      if (panel) existing.webContents.send('picto:settings:navigate', panel);
      return;
    }
    const win = createWindow(label, null, 900, 650);
    if (panel) {
      win.webContents.once('did-finish-load', () => {
        if (!win.isDestroyed()) win.webContents.send('picto:settings:navigate', panel);
      });
    }
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
    const hasParent = Boolean(mainWin && !mainWin.isDestroyed());
    const useTransparentManager = isMac || isWin;
    const win = new BrowserWindow({
      width: 700,
      height: 550,
      minWidth: 600,
      minHeight: 400,
      resizable: false,
      maximizable: false,
      fullscreenable: false,
      frame: false,
      transparent: useTransparentManager,
      backgroundColor: useTransparentManager ? '#00000000' : getThemeInfo(getCachedConfig).bgColor,
      ...(hasParent ? { parent: mainWin, modal: true } : {}),
      show: false,
      webPreferences: {
        preload: path.join(__dirname, 'preload.cjs'),
        contextIsolation: true,
        nodeIntegration: false,
        sandbox: true,
      },
    });

    win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
    windowsByLabel.set(label, win);
    const forcedShowTimer = setTimeout(() => {
      if (!win.isDestroyed() && !win.isVisible()) win.show();
    }, 5000);
    win.once('show', () => clearTimeout(forcedShowTimer));
    win.on('closed', () => {
      clearTimeout(forcedShowTimer);
      windowsByLabel.delete(label);
    });

    if (isDev) {
      void win.loadURL(`${DEV_URL}/library-manager.html`);
      if (shouldOpenDevTools) {
        win.webContents.openDevTools({ mode: 'detach', activate: false });
      }
    } else {
      void win.loadFile(path.join(__dirname, '..', 'dist', 'library-manager.html'));
    }
  }


  return {
    calcDetailWindowAspectRatio,
    calcDetailWindowSize: (imgW, imgH) => calcDetailWindowSize(screen, imgW, imgH),
    cancelAuthSession: authSessions.cancelAuthSession,
    getAuthSessionState: authSessions.getAuthSessionState,
    createWindow,
    getAllWindows,
    getWindow,
    openLibraryManager,
    openSettingsWindow,
    openSubscriptionsWindow,
    ownsWebContents,
    saveManualOnlyFansCredential: authSessions.saveManualOnlyFansCredential,
    startAuthSession: authSessions.startAuthSession,
    getMainWindow,
    sendToAllWindows,
    sendToFocusedWindow,
    sendToMainWindow,
    setThemePreference,
  };
}
