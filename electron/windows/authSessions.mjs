/**
 * Embedded auth sessions for Pixiv OAuth, booru API keys, and allowlisted
 * gallery-dl browser cookies. All window primitives arrive injected so this
 * module stays electron-import-free.
 */

const BOORU_AUTH_SITES = Object.freeze({
  gelbooru: Object.freeze({
    id: 'gelbooru',
    label: 'Gelbooru',
    loginUrl: 'https://gelbooru.com/index.php?code=00&page=account&s=login',
    optionsUrl: 'https://gelbooru.com/index.php?page=account&s=options',
  }),
  rule34: Object.freeze({
    id: 'rule34',
    label: 'Rule34.xxx',
    loginUrl: 'https://rule34.xxx/index.php?code=00&page=account&s=login',
    optionsUrl: 'https://rule34.xxx/index.php?page=account&s=options',
  }),
});

const COOKIE_AUTH_SITES = Object.freeze({
  hentaifoundry: Object.freeze({
    id: 'hentaifoundry',
    label: 'Hentai Foundry',
    loginUrl: 'https://www.hentai-foundry.com/site/index',
    cookieUrl: 'https://www.hentai-foundry.com',
    cookieNames: Object.freeze(['PHPSESSID']),
    requireLogoutLink: true,
  }),
  furaffinity: Object.freeze({
    id: 'furaffinity',
    label: 'Fur Affinity',
    loginUrl: 'https://www.furaffinity.net/login/',
    cookieUrl: 'https://www.furaffinity.net',
    cookieNames: Object.freeze(['a', 'b']),
  }),
});

const SUPPORTED_AUTH_SITES = new Set([
  'pixiv',
  'pixivuser',
  ...Object.keys(BOORU_AUTH_SITES),
  ...Object.keys(COOKIE_AUTH_SITES),
]);

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

  async function teardownAuthSessionWindow({ clearStorage = false } = {}) {
    const popup = authSession.popup;
    authSession.popup = null;
    authSession.partition = null;
    authSession.closing = false;
    if (!popup || popup.isDestroyed()) return;
    if (clearStorage) {
      try {
        await popup.webContents.session.clearStorageData();
      } catch {}
    }
    try {
      authSession.closing = true;
      popup.close();
    } catch {}
  }

  function createAuthSessionWindow(site, title) {
    const win = getMainWindow();
    // A stable site profile lets browser verification survive retries. The
    // authenticated session is cleared after Picto captures the credential.
    const partition = `persist:picto-auth-v1-${site}`;
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
        sandbox: true,
        partition,
      },
    });
    popup.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
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
    await teardownAuthSessionWindow({ clearStorage: true });
    return authSession.state;
  }

  async function inspectBooruPage(webContents) {
    return webContents.executeJavaScript(`
      (() => {
        const text = document.body ? document.body.innerText : '';
        const href = location.href;
        const hasCookieConsent = /agree to the usage of cookies according to our cookie policy/i.test(text);
        const cookieConsentButton = hasCookieConsent
          ? Array.from(document.querySelectorAll('button, a, [role="button"], input[type="button"], input[type="submit"]'))
              .find((element) => {
                const label = (element.textContent || element.value || '').trim().toLowerCase();
                return label === 'accept';
              })
          : null;
        if (cookieConsentButton) cookieConsentButton.click();
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
        const validApiKey = (value) => /^[a-f0-9]{16,}$/i.test((value || '').trim());
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
        const inputApiKey = readInput('api_key', 'api-key');
        const apiKey = validApiKey(inputApiKey)
          ? inputApiKey
          : apiKeyText || combinedCredential?.apiKey || null;
        const userId = readInput('user_id', 'user-id') || userIdText || combinedCredential?.userId || null;
        const hasLogoutLink = Array.from(document.querySelectorAll('a')).some((anchor) => {
          const label = (anchor.textContent || '')
            .trim()
            .toLowerCase()
            .replace(/[^a-z]+/g, ' ')
            .trim();
          return label === 'logout' || label.endsWith(' logout');
        });
        const hasAccountHome = /account home/i.test(text);
        const hasAccountOptions = /account options/i.test(text);
        const hasChallenge = Boolean(
          document.querySelector('#challenge-running, #challenge-stage, .cf-challenge')
        ) || /just a moment|verify (?:that )?you are human|checking your browser|cloudflare/i.test(
          [document.title || '', text].join('\\n')
        );
        return {
          href,
          title: document.title || null,
          hasLoginForm,
          hasLogoutLink,
          hasAccountHome,
          hasAccountOptions,
          hasChallenge,
          cookieConsentAccepted: Boolean(cookieConsentButton),
          apiKey,
          userId,
        };
      })();
    `, true);
  }

  async function startBooruSession(site) {
    const popup = authSession.popup;
    if (!popup || popup.isDestroyed()) throw new Error('Auth popup is unavailable.');
    const authContents = popup.webContents;
    let navigatingToOptions = false;
    let completed = false;
    let inspectionRunning = false;
    let inspectionTimer = null;

    const isOptionsUrl = (url) => /[?&]page=account(?:&|$)/i.test(url || '')
      && /[?&]s=options(?:&|$)/i.test(url || '');
    const isAuthenticatedAccountUrl = (url) => /[?&]page=account(?:&|$)/i.test(url || '')
      && /[?&]s=(?:home|profile)(?:&|$)/i.test(url || '');

    const navigateToOptions = async (url) => {
      if (completed || navigatingToOptions) return;
      navigatingToOptions = true;
      emitAuthSessionState({
        status: 'loading',
        current_url: url,
        message: `Authenticated. Reading ${site.label} API credentials…`,
      });
      try {
        popup.hide();
      } catch {}
      await authContents.loadURL(site.optionsUrl);
    };

    const inspectAndAdvance = async () => {
      if (completed || inspectionRunning || authSession.popup !== popup || authContents.isDestroyed()) return;
      inspectionRunning = true;
      try {
        const currentUrl = authContents.getURL();
        if (isAuthenticatedAccountUrl(currentUrl) && !isOptionsUrl(currentUrl)) {
          await navigateToOptions(currentUrl);
          return;
        }
        const result = await inspectBooruPage(authContents);
        if (result.cookieConsentAccepted) {
          emitAuthSessionState({
            status: 'active',
            current_url: result.href ?? authContents.getURL(),
            title: result.title ?? authSession.state.title,
            message: `Preparing ${site.label} login…`,
          });
          return;
        }
        const isOptionsPage = result.hasAccountOptions || isOptionsUrl(result.href);
        const looksAuthenticated = result.hasLogoutLink || result.hasAccountHome;
        if (result.hasChallenge) {
          emitAuthSessionState({
            status: 'active',
            current_url: result.href ?? authContents.getURL(),
            title: result.title ?? authSession.state.title,
            message: `${site.label} is asking for a browser check. Complete it in the login window; Picto will continue automatically.`,
          });
          return;
        }
        emitAuthSessionState({
          status: 'active',
          current_url: result?.href ?? authContents.getURL(),
          title: result?.title ?? authSession.state.title,
          message: !looksAuthenticated && result.hasLoginForm
            ? `Log in with ${site.label} to continue.`
            : navigatingToOptions
              ? `Reading ${site.label} account settings…`
              : looksAuthenticated
                ? `Authenticated. Reading ${site.label} API credentials…`
                : `Waiting for ${site.label} login…`,
        });
        if (looksAuthenticated && !isOptionsPage && !navigatingToOptions) {
          await navigateToOptions(result.href ?? currentUrl);
          return;
        }
        if (result.apiKey && result.userId) {
          completed = true;
          await completeAuthSession({
            site_category: site.id,
            credential_type: 'api_key',
            username: result.userId,
            password: result.apiKey,
          }, `${site.label} API key captured.`);
          return;
        }
        if (isOptionsPage && navigatingToOptions) {
          try {
            if (!popup.isDestroyed()) popup.show();
          } catch {}
          emitAuthSessionState({
            status: 'error',
            current_url: result.href ?? authContents.getURL(),
            title: result.title ?? authSession.state.title,
            message: `Logged in, but Picto could not find ${site.label} user_id and api_key on account settings.`,
            credential: null,
          });
        }
      } catch (err) {
        emitAuthSessionState({
          status: 'error',
          current_url: authContents.getURL(),
          title: authSession.state.title,
          message: err instanceof Error ? err.message : `Failed to inspect ${site.label} login state.`,
          credential: null,
        });
      } finally {
        inspectionRunning = false;
      }
    };

    popup.on('closed', () => {
      if (inspectionTimer != null) clearInterval(inspectionTimer);
      inspectionTimer = null;
    });

    authContents.on('page-title-updated', (_event, title) => {
      emitAuthSessionState({ title });
    });
    authContents.on('did-navigate', (_event, url) => {
      emitAuthSessionState({ status: 'loading', current_url: url, message: `Checking ${site.label} login state…` });
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
    authContents.on('did-fail-load', (_event, errorCode, errorDescription, validatedUrl, isMainFrame) => {
      if (isMainFrame === false || errorCode === -3) return;
      try {
        if (!popup.isDestroyed()) popup.show();
      } catch {}
      emitAuthSessionState({
        status: 'error',
        current_url: validatedUrl || authContents.getURL(),
        message: `${site.label} could not load in the login window (${errorDescription || `error ${errorCode}`}). Retry login; if the site shows a browser check, complete it in this window.`,
        credential: null,
      });
    });

    await authContents.loadURL(site.loginUrl);
    inspectionTimer = setInterval(() => { void inspectAndAdvance(); }, 750);
    emitAuthSessionState({
      status: 'active',
      current_url: site.loginUrl,
      message: `Log in with ${site.label} in the popup window. Picto will finish the account-settings step automatically after authentication.`,
    });
    popup.focus();
    return authSession.state;
  }

  async function startCookieSession(site) {
    const popup = authSession.popup;
    if (!popup || popup.isDestroyed()) throw new Error('Auth popup is unavailable.');
    const authContents = popup.webContents;
    let completed = false;

    const inspectCookies = async () => {
      if (completed || authSession.popup !== popup || authContents.isDestroyed()) return;
      try {
        if (site.requireLogoutLink) {
          let hasLogoutLink = false;
          try {
            hasLogoutLink = await authContents.executeJavaScript(`
              Array.from(document.querySelectorAll('a')).some((anchor) => {
                const label = (anchor.textContent || '').trim().toLowerCase();
                const href = (anchor.getAttribute('href') || '').toLowerCase();
                return label === 'logout' || href.includes('/logout');
              })
            `, true);
          } catch {
            return;
          }
          if (!hasLogoutLink) {
            emitAuthSessionState({
              status: 'active',
              current_url: authContents.getURL(),
              message: `Log in with ${site.label} to continue.`,
            });
            return;
          }
        }

        const stored = await authContents.session.cookies.get({ url: site.cookieUrl });
        const values = new Map(stored.map((cookie) => [cookie.name, cookie.value]));
        const cookies = Object.fromEntries(
          site.cookieNames
            .map((name) => [name, (values.get(name) || '').trim()])
            .filter(([, value]) => value),
        );
        const missing = site.cookieNames.filter((name) => !cookies[name]);
        if (missing.length > 0) {
          emitAuthSessionState({
            status: 'active',
            current_url: authContents.getURL(),
            message: `Log in with ${site.label} to continue.`,
          });
          return;
        }

        completed = true;
        await completeAuthSession({
          site_category: site.id,
          credential_type: 'cookies',
          cookies,
        }, `${site.label} session captured.`);
      } catch (err) {
        emitAuthSessionState({
          status: 'error',
          current_url: authContents.getURL(),
          message: err instanceof Error ? err.message : `Failed to inspect ${site.label} login state.`,
          credential: null,
        });
      }
    };

    authContents.on('page-title-updated', (_event, title) => {
      emitAuthSessionState({ title });
    });
    authContents.on('did-navigate', (_event, url) => {
      emitAuthSessionState({ status: 'loading', current_url: url });
      void inspectCookies();
    });
    authContents.on('did-navigate-in-page', (_event, url) => {
      emitAuthSessionState({ status: 'loading', current_url: url });
      void inspectCookies();
    });
    authContents.on('did-finish-load', () => {
      emitAuthSessionState({ status: 'active', current_url: authContents.getURL() });
      void inspectCookies();
    });

    await authContents.loadURL(site.loginUrl);
    emitAuthSessionState({
      status: 'active',
      current_url: site.loginUrl,
      message: `Log in with ${site.label} in the popup window. Picto stores only the session cookies gallery-dl requires.`,
    });
    popup.focus();
    return authSession.state;
  }

  async function startAuthSession(siteCategory, startUrl = null) {
    await cancelAuthSession();
    const requestedSite = String(siteCategory || '').trim().toLowerCase();
    if (!requestedSite) throw new Error('Missing site_category');
    if (!SUPPORTED_AUTH_SITES.has(requestedSite)) {
      throw new Error(`Unsupported auth site: ${requestedSite}`);
    }

    // Pixiv search and user queries share one credential owner and one OAuth flow.
    const site = requestedSite === 'pixivuser' ? 'pixiv' : requestedSite;

    const title = site === 'pixiv'
      ? 'Pixiv Login'
      : `Login: ${BOORU_AUTH_SITES[site]?.label ?? COOKIE_AUTH_SITES[site]?.label ?? site}`;
    const popup = createAuthSessionWindow(site, title);
    const authContents = popup.webContents;
    emitAuthSessionState({
      site_category: site,
      status: 'starting',
      title,
      current_url: startUrl,
      message: BOORU_AUTH_SITES[site]
        ? `Log in in the popup window, then Picto will read the ${BOORU_AUTH_SITES[site].label} account options page for user_id and api_key.`
        : COOKIE_AUTH_SITES[site]
          ? `Log in in the popup window. Picto will capture only the cookies required by ${COOKIE_AUTH_SITES[site].label}.`
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

    if (BOORU_AUTH_SITES[site]) {
      return startBooruSession(BOORU_AUTH_SITES[site]);
    }

    if (COOKIE_AUTH_SITES[site]) {
      return startCookieSession(COOKIE_AUTH_SITES[site]);
    }

    throw new Error(`Unsupported auth site: ${site}`);
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

  return {
    startAuthSession,
    cancelAuthSession,
  };
}
