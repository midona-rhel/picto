import fs from 'node:fs';
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

/** Try to read the theme from the last library's settings.json synchronously. */
function getThemeInfo(getCachedConfig) {
  let theme = 'dark';
  try {
    const config = getCachedConfig();
    const libraryPath = config?.lastLibrary;
    if (libraryPath) {
      const settingsPath = libraryPath + '/settings.json';
      const raw = fs.readFileSync(settingsPath, 'utf-8');
      const settings = JSON.parse(raw);
      theme = settings.colorScheme || settings.theme || 'dark';
    }
  } catch {}
  if (theme === 'auto') {
    // Resolve auto at creation time — CSS handles the rest
    try {
      const { nativeTheme } = esmRequire('electron');
      theme = nativeTheme.shouldUseDarkColors ? 'dark' : 'light';
    } catch {
      theme = 'dark';
    }
  }
  const bgColor = THEME_BG_COLORS[theme] || THEME_BG_COLORS.dark;
  return { theme, bgColor };
}

// Keep backward compat
function getThemeBgColor(getCachedConfig) {
  return getThemeInfo(getCachedConfig).bgColor;
}

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
  let authSession = {
    popup: null,
    partition: null,
    closing: false,
    state: {
      site_category: null,
      status: 'idle',
      title: null,
      current_url: null,
      message: null,
      credential: null,
    },
  };

  function emitAuthSessionState(patch) {
    authSession.state = {
      ...authSession.state,
      ...patch,
    };
    for (const win of BrowserWindow.getAllWindows()) {
      if (!win.isDestroyed()) win.webContents.send('auth:session-state', authSession.state);
    }
    return authSession.state;
  }

  function getMainWindow() {
    const win = windowsByLabel.get('main');
    return win && !win.isDestroyed() ? win : null;
  }

  function getEmbeddedAuthUserAgent() {
    return [
      'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)',
      'AppleWebKit/537.36 (KHTML, like Gecko)',
      'Chrome/137.0.0.0',
      'Safari/537.36',
    ].join(' ');
  }

  async function teardownAuthSessionWindow() {
    const popup = authSession.popup;
    authSession.popup = null;
    authSession.partition = null;
    authSession.closing = false;
    if (!popup || popup.isDestroyed()) return;
    try {
      await popup.webContents.session.clearStorageData();
    } catch {}
    try {
      authSession.closing = true;
      popup.close();
    } catch {}
  }

  function createAuthSessionWindow(site, title) {
    const win = getMainWindow();
    const partition = `picto-auth-${site}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    const popup = new BrowserWindow({
      width: 520,
      height: 760,
      minWidth: 420,
      minHeight: 640,
      title,
      backgroundColor: '#ffffff',
      autoHideMenuBar: true,
      show: false,
      ...(win && !win.isDestroyed() ? { parent: win } : {}),
      webPreferences: {
        contextIsolation: true,
        nodeIntegration: false,
        sandbox: false,
        partition,
      },
    });
    popup.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
    popup.webContents.setUserAgent(getEmbeddedAuthUserAgent());
    popup.once('ready-to-show', () => {
      if (!popup.isDestroyed()) popup.show();
    });
    popup.on('closed', () => {
      const shouldEmitCancelled = !authSession.closing && authSession.popup === popup;
      authSession.popup = null;
      authSession.partition = null;
      authSession.closing = false;
      if (shouldEmitCancelled) {
        emitAuthSessionState({
          site_category: null,
          status: 'cancelled',
          title: null,
          current_url: null,
          message: 'Login window closed.',
          credential: null,
        });
      }
    });
    authSession.popup = popup;
    authSession.partition = partition;
    return popup;
  }

  async function completeAuthSession(credential, message = 'Login completed.') {
    emitAuthSessionState({
      status: 'completed',
      message,
      credential,
    });
    await teardownAuthSessionWindow();
    return authSession.state;
  }

  async function scrapeGelbooruCredential(webContents) {
    return webContents.executeJavaScript(`
      (() => {
        const collect = () => {
          const inputs = Array.from(document.querySelectorAll('input'));
          const findByName = (...names) => {
            for (const input of inputs) {
              const key = (input.name || input.id || '').toLowerCase();
              if (names.some((name) => key.includes(name))) {
                const value = (input.value || '').trim();
                if (value) return value;
              }
            }
            return null;
          };
          const text = document.body ? document.body.innerText : '';
          const apiKeyFromText = text.match(/api[_ -]?key\\s*[:\\n]\\s*([a-f0-9]{16,})/i)?.[1] ?? null;
          const userIdFromText = text.match(/user[_ -]?id\\s*[:\\n]\\s*(\\d+)/i)?.[1] ?? null;
          return {
            apiKey: findByName('api_key', 'api-key') || apiKeyFromText,
            userId: findByName('user_id', 'user-id') || userIdFromText,
            href: location.href,
            title: document.title || null,
          };
        };
        return collect();
      })();
    `, true);
  }

  async function inspectNamedCookies(webContents, url, names) {
    const cookies = await webContents.session.cookies.get({ url });
    const values = {};
    for (const name of names) {
      const value = (cookies.find((cookie) => cookie.name === name)?.value || '').trim();
      if (value) values[name] = value;
    }
    return {
      hasAll: names.every((name) => Boolean(values[name])),
      values,
    };
  }

  async function inspectGelbooruPage(webContents) {
    return webContents.executeJavaScript(`
      (() => {
        const text = document.body ? document.body.innerText : '';
        const href = location.href;
        const hasLoginForm = Boolean(
          document.querySelector('input[name="user"], input[name="username"], input[type="password"]')
        );
        const apiKeyText = text.match(/api[_ -]?key\\s*[:\\n]\\s*([a-f0-9]{16,})/i)?.[1] ?? null;
        const userIdText = text.match(/user[_ -]?id\\s*[:\\n]\\s*(\\d+)/i)?.[1] ?? null;
        const inputs = Array.from(document.querySelectorAll('input, textarea'));
        const readInput = (...needles) => {
          for (const input of inputs) {
            const key = (input.name || input.id || '').toLowerCase();
            if (needles.some((needle) => key.includes(needle))) {
              const value = (input.value || '').trim();
              if (value) return value;
            }
          }
          return null;
        };
        const snippets = [];
        snippets.push(text);
        snippets.push(href);
        for (const input of inputs) {
          const value = (input.value || '').trim();
          if (value) snippets.push(value);
        }
        for (const anchor of Array.from(document.querySelectorAll('a[href]'))) {
          const value = (anchor.getAttribute('href') || '').trim();
          if (value) snippets.push(value);
        }
        const combinedCredential = snippets
          .map((value) => {
            const apiKey = value.match(/api[_-]?key=([a-f0-9]{16,})/i)?.[1] ?? null;
            const userId = value.match(/user[_-]?id=(\\d+)/i)?.[1] ?? null;
            return apiKey ? { apiKey, userId } : null;
          })
          .find(Boolean) ?? null;
        const apiKey = readInput('api_key', 'api-key') || apiKeyText || combinedCredential?.apiKey || null;
        const userId = readInput('user_id', 'user-id') || userIdText || combinedCredential?.userId || null;
        const hasLogoutLink = Array.from(document.querySelectorAll('a')).some((anchor) => {
          const label = (anchor.textContent || '').trim().toLowerCase();
          return label === 'logout';
        });
        const hasAccountHome = /account home/i.test(text);
        const hasAccountOptions = /account options/i.test(text);
        return {
          href,
          title: document.title || null,
          hasLoginForm,
          hasLogoutLink,
          hasAccountHome,
          hasAccountOptions,
          apiKey,
          userId,
        };
      })();
    `, true);
  }

  async function inspectGelbooruCookies(webContents) {
    const cookies = await webContents.session.cookies.get({ url: 'https://gelbooru.com' });
    const names = new Map(cookies.map((cookie) => [cookie.name, cookie.value]));
    const userId = (names.get('user_id') || '').trim();
    const passHash = (names.get('pass_hash') || '').trim();
    return {
      hasAuthCookies: Boolean(userId && passHash),
      userId,
      passHash,
    };
  }

  async function inspectBooruCookies(webContents, url) {
    const cookies = await webContents.session.cookies.get({ url });
    const names = new Map(cookies.map((cookie) => [cookie.name, cookie.value]));
    const userId = (names.get('user_id') || '').trim();
    const passHash = (names.get('pass_hash') || '').trim();
    return {
      hasAuthCookies: Boolean(userId && passHash),
      userId,
      passHash,
    };
  }

  async function startBooruApiKeySession({
    site,
    loginUrl,
    optionsUrl,
    cookieUrl,
    successMessage,
  }) {
    const popup = authSession.popup;
    if (!popup || popup.isDestroyed()) throw new Error('Auth popup is unavailable.');
    const authContents = popup.webContents;
    let navigatingToOptions = false;
    let completed = false;

    const inspectAndAdvance = async () => {
      if (completed || authSession.popup !== popup || authContents.isDestroyed()) return;
      try {
        const result = await inspectGelbooruPage(authContents);
        const cookieState = await inspectBooruCookies(authContents, cookieUrl);
        const looksAuthenticated = cookieState.hasAuthCookies || result.hasLogoutLink || result.hasAccountHome;
        const effectiveUserId = result?.userId || cookieState.userId || null;
        emitAuthSessionState({
          status: 'active',
          current_url: result?.href ?? authContents.getURL(),
          title: result?.title ?? authSession.state.title,
          message: !looksAuthenticated && result.hasLoginForm
            ? `Log in with ${site} to continue.`
            : navigatingToOptions
              ? `Loading ${site} account settings…`
              : `Authenticated. Reading ${site} API credentials…`,
        });
        if (looksAuthenticated && (!result.apiKey || !effectiveUserId) && !navigatingToOptions) {
          navigatingToOptions = true;
          try {
            popup.hide();
          } catch {}
          await authContents.loadURL(optionsUrl);
          return;
        }
        if (result?.apiKey && effectiveUserId) {
          completed = true;
          await completeAuthSession({
            site_category: site,
            credential_type: 'api_key',
            username: effectiveUserId,
            password: result.apiKey,
          }, successMessage);
          return;
        }
        if (looksAuthenticated && navigatingToOptions) {
          try {
            if (!popup.isDestroyed()) popup.show();
          } catch {}
          emitAuthSessionState({
            status: 'error',
            current_url: result?.href ?? authContents.getURL(),
            title: result?.title ?? authSession.state.title,
            message: `Logged in, but Picto could not find ${site} user_id and api_key on account settings.`,
            credential: null,
          });
        }
      } catch (err) {
        emitAuthSessionState({
          status: 'error',
          current_url: authContents.getURL(),
          title: authSession.state.title,
          message: err instanceof Error ? err.message : `Failed to inspect ${site} login state.`,
          credential: null,
        });
      }
    };

    authContents.on('page-title-updated', (_event, title) => {
      emitAuthSessionState({ title });
    });
    authContents.on('did-navigate', (_event, url) => {
      emitAuthSessionState({ status: 'loading', current_url: url, message: `Checking ${site} login state…` });
      void inspectAndAdvance();
    });
    authContents.on('did-navigate-in-page', (_event, url) => {
      emitAuthSessionState({ status: 'loading', current_url: url });
      void inspectAndAdvance();
    });
    authContents.on('did-finish-load', () => {
      emitAuthSessionState({
        status: 'active',
        current_url: authContents.getURL(),
        title: authSession.state.title,
      });
      void inspectAndAdvance();
    });

    await authContents.loadURL(loginUrl);
    emitAuthSessionState({
      status: 'active',
      current_url: loginUrl,
      message: `Log in with ${site} in the popup window. Picto will finish the account-settings step automatically after authentication.`,
    });
    popup.focus();
    return authSession.state;
  }

  async function startCookieSession({
    site,
    loginUrl,
    cookieUrl,
    cookieNames,
    successMessage,
  }) {
    const popup = authSession.popup;
    if (!popup || popup.isDestroyed()) throw new Error('Auth popup is unavailable.');
    const authContents = popup.webContents;
    let completed = false;

    const inspectAndComplete = async () => {
      if (completed || authSession.popup !== popup || authContents.isDestroyed()) return;
      try {
        const cookieState = await inspectNamedCookies(authContents, cookieUrl, cookieNames);
        emitAuthSessionState({
          status: 'active',
          current_url: authContents.getURL(),
          title: authContents.getTitle() || authSession.state.title,
          message: cookieState.hasAll
            ? `Authenticated. Saving ${site} session cookies…`
            : `Log in with ${site} to continue.`,
        });
        if (!cookieState.hasAll) return;
        completed = true;
        await completeAuthSession({
          site_category: site,
          credential_type: 'cookies',
          cookies: cookieState.values,
        }, successMessage);
      } catch (err) {
        emitAuthSessionState({
          status: 'error',
          current_url: authContents.getURL(),
          title: authSession.state.title,
          message: err instanceof Error ? err.message : `Failed to inspect ${site} cookies.`,
          credential: null,
        });
      }
    };

    authContents.on('page-title-updated', (_event, title) => {
      emitAuthSessionState({ title });
    });
    authContents.on('did-navigate', (_event, url) => {
      emitAuthSessionState({ status: 'loading', current_url: url, message: `Checking ${site} session…` });
      void inspectAndComplete();
    });
    authContents.on('did-navigate-in-page', (_event, url) => {
      emitAuthSessionState({ status: 'loading', current_url: url });
      void inspectAndComplete();
    });
    authContents.on('did-finish-load', () => {
      emitAuthSessionState({
        status: 'active',
        current_url: authContents.getURL(),
        title: authContents.getTitle() || authSession.state.title,
      });
      void inspectAndComplete();
    });

    await authContents.loadURL(loginUrl);
    emitAuthSessionState({
      status: 'active',
      current_url: loginUrl,
      message: `Log in with ${site} in the popup window. Picto will save the session cookies gallery-dl needs.`,
    });
    popup.focus();
    return authSession.state;
  }

  async function startAuthSession(siteCategory, startUrl = null) {
    await cancelAuthSession();
    const site = String(siteCategory || '').trim().toLowerCase();
    if (!site) throw new Error('Missing site_category');

    const title = site === 'pixiv' ? 'Pixiv Login' : `Login: ${site}`;
    const popup = createAuthSessionWindow(site, title);
    const authContents = popup.webContents;
    emitAuthSessionState({
      site_category: site,
      status: 'starting',
      title,
      current_url: startUrl,
      message: site === 'gelbooru'
        ? 'Log in in the popup window, then Picto will read the account options page for user_id and api_key.'
        : 'Waiting for login…',
      credential: null,
    });

    if (site === 'pixiv') {
      const extractCode = (url) => {
        try {
          const parsed = new URL(url);
          return parsed.searchParams.get('code') || null;
        } catch {
          return null;
        }
      };

      const handlePixivCallback = async (url) => {
        const code = extractCode(url);
        if (!code) {
          emitAuthSessionState({ status: 'error', current_url: url, message: 'No code in Pixiv callback.', credential: null });
          await teardownAuthSessionWindow();
          return;
        }
        let phpsessid = null;
        try {
          const cookies = await authContents.session.cookies.get({ domain: '.pixiv.net', name: 'PHPSESSID' });
          if (cookies.length > 0) phpsessid = cookies[0].value;
        } catch {}
        await completeAuthSession({
          site_category: 'pixiv',
          credential_type: 'oauth_token',
          oauth_code: code,
          phpsessid,
        }, 'Pixiv authorization completed.');
      };

      authContents.on('page-title-updated', (_event, title) => {
        emitAuthSessionState({ title });
      });
      authContents.on('will-redirect', (event, url) => {
        emitAuthSessionState({ status: 'loading', current_url: url, message: 'Following Pixiv redirect…' });
        if (url.startsWith('pixiv://')) {
          event.preventDefault();
          void handlePixivCallback(url);
        }
      });
      authContents.on('will-navigate', (event, url) => {
        emitAuthSessionState({ status: 'loading', current_url: url });
        if (url.startsWith('pixiv://')) {
          event.preventDefault();
          void handlePixivCallback(url);
        }
      });
      await authContents.loadURL(startUrl);
      emitAuthSessionState({ status: 'active', current_url: startUrl, message: 'Sign in with Pixiv in the popup window.' });
      popup.focus();
      return authSession.state;
    }

    if (site === 'gelbooru') {
      return startBooruApiKeySession({
        site: 'gelbooru',
        loginUrl: startUrl || 'https://gelbooru.com/index.php?code=00&page=account&s=login',
        optionsUrl: 'https://gelbooru.com/index.php?page=account&s=options',
        cookieUrl: 'https://gelbooru.com',
        successMessage: 'Gelbooru API key captured.',
      });
    }

    if (site === 'rule34') {
      return startBooruApiKeySession({
        site: 'rule34',
        loginUrl: startUrl || 'https://rule34.xxx/index.php?code=00&page=account&s=login',
        optionsUrl: 'https://rule34.xxx/index.php?page=account&s=options',
        cookieUrl: 'https://rule34.xxx',
        successMessage: 'Rule34 API key captured.',
      });
    }

    if (site === 'twitter') {
      return startCookieSession({
        site: 'twitter',
        loginUrl: startUrl || 'https://x.com/i/flow/login',
        cookieUrl: 'https://x.com',
        cookieNames: ['auth_token', 'ct0'],
        successMessage: 'Twitter/X cookies captured.',
      });
    }

    if (site === 'furaffinity') {
      return startCookieSession({
        site: 'furaffinity',
        loginUrl: startUrl || 'https://www.furaffinity.net/login/',
        cookieUrl: 'https://www.furaffinity.net',
        cookieNames: ['a', 'b'],
        successMessage: 'FurAffinity cookies captured.',
      });
    }

    emitAuthSessionState({
      status: 'error',
      message: `Popup auth is not implemented for ${site}.`,
      credential: null,
    });
    await teardownAuthSessionWindow();
    return authSession.state;
  }

  async function cancelAuthSession() {
    await teardownAuthSessionWindow();
    emitAuthSessionState({
      site_category: null,
      status: 'idle',
      title: null,
      current_url: null,
      message: null,
      credential: null,
    });
    return null;
  }

  function setAuthSessionBounds(_bounds) {
    return null;
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

  function createWindow(label = 'main', hash = null, width = MAIN_WINDOW_DEFAULT_WIDTH, height = MAIN_WINDOW_DEFAULT_HEIGHT) {
    const isSettings = label === 'settings';
    const isSubscriptions = label === 'subscriptions';
    const isDetail = hash != null && !isSettings && !isSubscriptions;
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
            resizable: false,
            maximizable: false,
            fullscreenable: false,
            frame: false,
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
        sandbox: false,
      },
    };
    if (isMain && savedMainState?.x != null && savedMainState?.y != null) {
      winOpts.x = savedMainState.x;
      winOpts.y = savedMainState.y;
    }

    const win = new BrowserWindow(winOpts);

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
      if (label === 'main') {
        clearAuthSessionView();
      }
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
      backgroundColor: getThemeBgColor(getCachedConfig),
      ...(mainWin && !mainWin.isDestroyed() ? { parent: mainWin } : {}),
      show: true,
      webPreferences: {
        preload: path.join(__dirname, 'preload.cjs'),
        contextIsolation: true,
        nodeIntegration: false,
        sandbox: false,
      },
    });

    win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
    windowsByLabel.set(label, win);
    win.on('closed', () => windowsByLabel.delete(label));

    if (isDev) {
      void win.loadURL(`${DEV_URL}/library-manager.html`);
      win.webContents.openDevTools({ mode: 'detach' });
    } else {
      void win.loadFile(path.join(__dirname, '..', 'dist', 'library-manager.html'));
    }
  }

  /**
   * Open a popup for Pixiv OAuth login.
   * Intercepts the pixiv:// callback redirect and extracts the auth code.
   * Returns a Promise that resolves with the code or rejects on cancel/error.
   */
  function openPixivOAuthPopup(loginUrl) {
    return new Promise((resolve, reject) => {
      const popup = new BrowserWindow({
        width: 500,
        height: 700,
        resizable: false,
        minimizable: false,
        maximizable: false,
        title: 'Pixiv Login',
        webPreferences: {
          nodeIntegration: false,
          contextIsolation: true,
        },
      });

      popup.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
      let resolved = false;

      const extractCode = (url) => {
        try {
          const parsed = new URL(url);
          return parsed.searchParams.get('code') || null;
        } catch {
          return null;
        }
      };

      const handlePixivCallback = async (url) => {
        resolved = true;
        const code = extractCode(url);
        if (!code) {
          reject(new Error('No code in Pixiv callback'));
          popup.close();
          return;
        }
        // Capture PHPSESSID cookie from the login session
        let phpsessid = null;
        try {
          const cookies = await popup.webContents.session.cookies.get({ domain: '.pixiv.net', name: 'PHPSESSID' });
          if (cookies.length > 0) phpsessid = cookies[0].value;
        } catch { /* best effort */ }
        resolve({ code, phpsessid });
        popup.close();
      };

      // Intercept redirects to pixiv:// scheme — prevent OS from handling it
      popup.webContents.on('will-redirect', (event, url) => {
        if (url.startsWith('pixiv://')) {
          event.preventDefault();
          handlePixivCallback(url);
        }
      });

      popup.webContents.on('will-navigate', (event, url) => {
        if (url.startsWith('pixiv://')) {
          event.preventDefault();
          handlePixivCallback(url);
        }
      });

      popup.on('closed', () => {
        if (!resolved) {
          reject(new Error('Pixiv login cancelled'));
        }
      });

      popup.loadURL(loginUrl);
    });
  }

  return {
    calcDetailWindowSize: (imgW, imgH) => calcDetailWindowSize(screen, imgW, imgH),
    cancelAuthSession,
    createWindow,
    getAllWindows,
    getWindow,
    openLibraryManager,
    openPixivOAuthPopup,
    openSettingsWindow,
    openSubscriptionsWindow,
    setAuthSessionBounds,
    startAuthSession,
    getMainWindow,
    sendToAllWindows,
    sendToFocusedWindow,
    sendToMainWindow,
  };
}
