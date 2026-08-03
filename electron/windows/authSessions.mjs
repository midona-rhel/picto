/**
 * Embedded auth sessions — cookie-login capture, booru API-key scraping,
 * and the Pixiv OAuth popup. Split from windowManager.mjs; all window
 * primitives arrive injected so this module stays electron-import-free.
 */

// Cookie-based browser-login sites. `cookieNames` are the cookies gallery-dl's
// extractor requires — their presence in the isolated auth session means the
// user finished logging in. `avoidUrlSubstrings` delays capture while the user
// is still on a login page (for sites whose session cookie exists pre-login).
// `expiryCookie` names the cookie whose expiration represents the session's
// real lifetime (default: first of cookieNames) — short-lived helpers like
// twitter's ct0 CSRF token must not drive the credential's expires_at.
const COOKIE_LOGIN_SITES = {
  twitter: {
    loginUrl: 'https://x.com/i/flow/login',
    cookieUrl: 'https://x.com',
    cookieNames: ['auth_token', 'ct0'],
    expiryCookie: 'auth_token',
    successMessage: 'Twitter/X cookies captured.',
  },
  furaffinity: {
    loginUrl: 'https://www.furaffinity.net/login/',
    cookieUrl: 'https://www.furaffinity.net',
    cookieNames: ['a', 'b'],
    successMessage: 'FurAffinity cookies captured.',
  },
  patreon: {
    loginUrl: 'https://www.patreon.com/login',
    cookieUrl: 'https://www.patreon.com',
    cookieNames: ['session_id'],
    successMessage: 'Patreon session captured.',
  },
  fanbox: {
    loginUrl: 'https://www.fanbox.cc/login',
    cookieUrl: 'https://www.fanbox.cc',
    cookieNames: ['FANBOXSESSID'],
    successMessage: 'Fanbox session captured.',
  },
  fantia: {
    loginUrl: 'https://fantia.jp/sessions/signin',
    cookieUrl: 'https://fantia.jp',
    cookieNames: ['_session_id'],
    successMessage: 'Fantia session captured.',
    avoidUrlSubstrings: ['/sessions/signin'],
  },
  instagram: {
    loginUrl: 'https://www.instagram.com/accounts/login/',
    cookieUrl: 'https://www.instagram.com',
    cookieNames: ['sessionid'],
    successMessage: 'Instagram session captured.',
  },
  deviantart: {
    loginUrl: 'https://www.deviantart.com/users/login',
    cookieUrl: 'https://www.deviantart.com',
    cookieNames: ['auth', 'auth_secure', 'userinfo'],
    successMessage: 'DeviantArt session captured.',
  },
  nijie: {
    loginUrl: 'https://nijie.info/login.php',
    cookieUrl: 'https://nijie.info',
    cookieNames: ['nijie_tok'],
    successMessage: 'Nijie session captured.',
  },
};

export function createAuthSessions({ BrowserWindow, getMainWindow }) {
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

  async function inspectNamedCookies(webContents, url, names, expiryCookie) {
    const cookies = await webContents.session.cookies.get({ url });
    const values = {};
    let expiresAt = null;
    const expiryName = expiryCookie || names[0];
    for (const name of names) {
      const cookie = cookies.find((c) => c.name === name);
      const value = (cookie?.value || '').trim();
      if (value) {
        values[name] = value;
        // expirationDate is epoch seconds; session cookies have none. Only the
        // designated session cookie's lifetime represents the login's expiry.
        if (name === expiryName && cookie.expirationDate) {
          expiresAt = new Date(cookie.expirationDate * 1000).toISOString();
        }
      }
    }
    return {
      hasAll: names.every((name) => Boolean(values[name])),
      values,
      expiresAt,
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
    expiryCookie,
    successMessage,
    avoidUrlSubstrings,
  }) {
    const popup = authSession.popup;
    if (!popup || popup.isDestroyed()) throw new Error('Auth popup is unavailable.');
    const authContents = popup.webContents;
    let completed = false;

    const inspectAndComplete = async () => {
      if (completed || authSession.popup !== popup || authContents.isDestroyed()) return;
      try {
        // Some sites (Rails apps like Fantia) set their session cookie for
        // anonymous visitors too — the cookie only proves login once the user
        // has left the login page.
        const currentUrl = authContents.getURL() || '';
        if ((avoidUrlSubstrings ?? []).some((needle) => currentUrl.includes(needle))) {
          emitAuthSessionState({
            status: 'active',
            current_url: currentUrl,
            title: authContents.getTitle() || authSession.state.title,
            message: `Log in with ${site} to continue.`,
          });
          return;
        }
        const cookieState = await inspectNamedCookies(authContents, cookieUrl, cookieNames, expiryCookie);
        emitAuthSessionState({
          status: 'active',
          current_url: currentUrl,
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
          expires_at: cookieState.expiresAt,
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

    const cookieLoginSpec = COOKIE_LOGIN_SITES[site];
    if (cookieLoginSpec) {
      return startCookieSession({
        site,
        loginUrl: startUrl || cookieLoginSpec.loginUrl,
        cookieUrl: cookieLoginSpec.cookieUrl,
        cookieNames: cookieLoginSpec.cookieNames,
        expiryCookie: cookieLoginSpec.expiryCookie,
        successMessage: cookieLoginSpec.successMessage,
        avoidUrlSubstrings: cookieLoginSpec.avoidUrlSubstrings,
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
    startAuthSession,
    cancelAuthSession,
    setAuthSessionBounds,
    openPixivOAuthPopup,
  };
}
