import { createHmac, randomUUID } from 'node:crypto';

/**
 * Embedded auth sessions for direct source-site login. Credential capture is
 * explicit because the backend owns the accepted credential formats; sources
 * without one still get a real-site browser session instead of being rejected.
 * All window primitives arrive injected so this module stays electron-import-free.
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

const IDOL_COMPLEX_LOGIN_URL = 'https://login.idol.sankakucomplex.com/oidc/auth?response_type=code&scope=openid&client_id=idol-web-app&redirect_uri=https%3A%2F%2Fwww.idolcomplex.com%2Fsso%2Fcallback&state=return_uri%3Dhttps%3A%2F%2Fwww.idolcomplex.com%2Fen%2Flogin&theme=black&route=login&lang=en';
const SANKAKU_LOGIN_URL = 'https://login.sankakucomplex.com/oidc/auth?response_type=code&scope=openid&client_id=sankaku-web-app&redirect_uri=https%3A%2F%2Fsankaku.app%2Fsso%2Fcallback&state=return_uri%3Dhttps%3A%2F%2Fsankaku.app%2F&theme=black&route=login&lang=en';
const OAUTH_CALLBACK_URL = 'https://picto.app/oauth/callback';
const OAUTH_AUTH_SITES = Object.freeze({
  baraag: Object.freeze({
    id: 'baraag',
    label: 'Baraag',
    loginUrl: 'https://baraag.net/auth/sign_in',
    registerUrl: 'https://baraag.net/api/v1/apps',
    authorizeUrl: 'https://baraag.net/oauth/authorize',
    tokenUrl: 'https://baraag.net/oauth/token',
  }),
  tumblr: Object.freeze({
    id: 'tumblr',
    label: 'Tumblr',
    loginUrl: 'https://www.tumblr.com/login',
    requestTokenUrl: 'https://www.tumblr.com/oauth/request_token',
    authorizeUrl: 'https://www.tumblr.com/oauth/authorize',
    tokenUrl: 'https://www.tumblr.com/oauth/access_token',
    consumerKey: 'O3hU2tMi5e4Qs5t3vezEi6L0qRORJ5y9oUpSGsrWu8iA3UCc3B',
    consumerSecret: 'sFdsK3PDdP2QpYMRAoq0oDnw0sFS24XigXmdfnaeNZpJpqAn03',
  }),
});

const COOKIE_AUTH_SITES = Object.freeze({
  hentaifoundry: Object.freeze({
    id: 'hentaifoundry',
    label: 'Hentai Foundry',
    loginUrl: 'https://www.hentai-foundry.com/site/index',
    cookieUrl: 'https://www.hentai-foundry.com',
    cookieNames: Object.freeze(['PHPSESSID']),
  }),
  furaffinity: Object.freeze({
    id: 'furaffinity',
    label: 'Fur Affinity',
    loginUrl: 'https://www.furaffinity.net/login/',
    cookieUrl: 'https://www.furaffinity.net',
    cookieNames: Object.freeze(['a', 'b']),
    authenticatedCookieNames: Object.freeze(['a', 'b']),
  }),
  danbooru: Object.freeze({
    id: 'danbooru',
    label: 'Danbooru',
    loginUrl: 'https://danbooru.donmai.us/session/new',
    cookieUrl: 'https://danbooru.donmai.us',
  }),
  webtoons: Object.freeze({
    id: 'webtoons',
    label: 'Webtoons',
    loginUrl: 'https://www.webtoons.com/member/login',
    cookieUrl: 'https://www.webtoons.com',
    unauthenticatedUrlPattern: /\/(?:member\/(?:login|join)|(?:[a-z]{2}\/)?age-gate)(?:[/?#]|$)/i,
  }),
  deviantart: Object.freeze({
    id: 'deviantart',
    label: 'DeviantArt',
    loginUrl: 'https://www.deviantart.com/users/login',
    cookieUrl: 'https://www.deviantart.com',
    unauthenticatedUrlPattern: /\/(?:users\/login|join)(?:[/?#]|$)/i,
  }),
  patreon: Object.freeze({
    id: 'patreon',
    label: 'Patreon',
    loginUrl: 'https://www.patreon.com/login?l=en-GB',
    cookieUrl: 'https://www.patreon.com',
    cookieNames: Object.freeze(['session_id']),
    authenticatedCookieNames: Object.freeze(['session_id']),
  }),
  fanbox: Object.freeze({
    id: 'fanbox',
    label: 'pixivFANBOX',
    loginUrl: 'https://accounts.pixiv.net/login?prompt=select_account&return_to=https%3A%2F%2Fwww.fanbox.cc%2Fauth%2Fstart&source=fanbox',
    cookieUrl: 'https://www.fanbox.cc',
    cookieNames: Object.freeze(['FANBOXSESSID']),
    authenticatedCookieNames: Object.freeze(['FANBOXSESSID']),
  }),
  subscribestar: Object.freeze({
    id: 'subscribestar',
    label: 'SubscribeStar',
    loginUrl: 'https://www.subscribestar.com/login',
    cookieUrl: 'https://www.subscribestar.com',
    cookieNames: Object.freeze(['_personalization_id']),
    authenticatedCookieNames: Object.freeze(['_personalization_id']),
  }),
  idolcomplex: Object.freeze({
    id: 'idolcomplex',
    label: 'Idol Complex',
    loginUrl: IDOL_COMPLEX_LOGIN_URL,
    cookieUrl: 'https://www.idolcomplex.com',
    cookieNames: Object.freeze(['accessToken', 'refreshToken', 'ssoLoginValid']),
    authenticatedCookieNames: Object.freeze(['accessToken', 'refreshToken', 'ssoLoginValid']),
  }),
  sankaku: Object.freeze({
    id: 'sankaku',
    label: 'Sankaku',
    loginUrl: SANKAKU_LOGIN_URL,
    cookieUrl: 'https://sankaku.app',
    cookieNames: Object.freeze(['accessToken', 'refreshToken', 'ssoLoginValid']),
    authenticatedCookieNames: Object.freeze(['accessToken', 'refreshToken', 'ssoLoginValid']),
  }),
  yandere: Object.freeze({ id: 'yandere', label: 'Yande.re', loginUrl: 'https://yande.re/user/login', cookieUrl: 'https://yande.re' }),
  konachan: Object.freeze({ id: 'konachan', label: 'Konachan', loginUrl: 'https://konachan.com/user/login', cookieUrl: 'https://konachan.com' }),
  safebooru: Object.freeze({ id: 'safebooru', label: 'Safebooru', loginUrl: 'https://safebooru.org/index.php?page=account&s=login&code=00', cookieUrl: 'https://safebooru.org' }),
  e621: Object.freeze({ id: 'e621', label: 'e621', loginUrl: 'https://e621.net/session/new', cookieUrl: 'https://e621.net' }),
});

const SUPPORTED_AUTH_SITES = new Set([
  'pixiv',
  'pixivuser',
  ...Object.keys(BOORU_AUTH_SITES),
  ...Object.keys(OAUTH_AUTH_SITES),
  ...Object.keys(COOKIE_AUTH_SITES),
]);

export function getStaticAuthLoginRoutes() {
  return Object.values({ ...BOORU_AUTH_SITES, ...OAUTH_AUTH_SITES, ...COOKIE_AUTH_SITES }).map(({ id, loginUrl }) => ({
    site: id,
    loginUrl,
  }));
}

function formBody(values) {
  return new URLSearchParams(values).toString();
}

function sanitizeAuthUserAgent(userAgent) {
  return String(userAgent || '')
    .replace(/\s+Electron\/[^\s]+/gi, '')
    .replace(/\s+Picto\/[^\s]+/gi, '')
    .replace(/\s{2,}/g, ' ')
    .trim();
}

function oauthEncode(value) {
  return encodeURIComponent(String(value))
    .replace(/[!'()*]/g, (character) => `%${character.charCodeAt(0).toString(16).toUpperCase()}`);
}

function oauth1AuthorizationHeader({ method, url, params, consumerKey, consumerSecret, token, tokenSecret }) {
  const oauthParams = {
    oauth_consumer_key: consumerKey,
    oauth_nonce: randomUUID().replaceAll('-', ''),
    oauth_signature_method: 'HMAC-SHA1',
    oauth_timestamp: String(Math.floor(Date.now() / 1000)),
    oauth_version: '1.0',
    ...(token ? { oauth_token: token } : {}),
  };
  const signatureParams = Object.entries({ ...params, ...oauthParams })
    .map(([key, value]) => [oauthEncode(key), oauthEncode(value)])
    .sort(([leftKey, leftValue], [rightKey, rightValue]) => leftKey.localeCompare(rightKey) || leftValue.localeCompare(rightValue))
    .map(([key, value]) => `${key}=${value}`)
    .join('&');
  const baseString = [method.toUpperCase(), url, signatureParams]
    .map(oauthEncode)
    .join('&');
  const signingKey = `${oauthEncode(consumerSecret)}&${oauthEncode(tokenSecret || '')}`;
  const signature = createHmac('sha1', signingKey).update(baseString).digest('base64');
  const headerParams = { ...oauthParams, oauth_signature: signature };
  return `OAuth ${Object.entries(headerParams)
    .map(([key, value]) => `${oauthEncode(key)}="${oauthEncode(value)}"`)
    .join(', ')}`;
}

async function responseJson(response, operation) {
  let body;
  try {
    body = await response.json();
  } catch {
    body = { error: await response.text().catch(() => '') };
  }
  if (!response.ok) {
    const detail = body?.error_description || body?.error || response.status;
    throw new Error(`${operation} failed: ${detail}`);
  }
  return body;
}

async function responseForm(response, operation) {
  const body = await response.text();
  if (!response.ok) throw new Error(`${operation} failed: HTTP ${response.status}`);
  const values = Object.fromEntries(new URLSearchParams(body));
  if (values.error) throw new Error(`${operation} failed: ${values.error}`);
  return values;
}

export function createAuthSessions({ BrowserWindow, getMainWindow, fetchImpl = fetch }) {
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
    const currentUserAgent = authSession.popup?.webContents?.getUserAgent?.()
      ?? popup.webContents.getUserAgent?.()
      ?? '';
    const sanitizedUserAgent = sanitizeAuthUserAgent(currentUserAgent);
    if (sanitizedUserAgent) {
      popup.webContents.setUserAgent(sanitizedUserAgent);
    }
    popup.webContents.setWindowOpenHandler(({ url }) => {
      // Keep target=_blank signup/OAuth links inside Picto's isolated auth
      // profile instead of discarding them or opening the system browser.
      if (/^https:\/\//i.test(url)) {
        queueMicrotask(() => {
          if (!popup.isDestroyed()) void popup.webContents.loadURL(url);
        });
      }
      return { action: 'deny' };
    });
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
    let inspectionTimer = null;

    const hasAuthenticatedDomSignal = async () => authContents.executeJavaScript(`
      (() => {
        const normalize = (value) => (value || '').replace(/\\s+/g, ' ').trim().toLowerCase();
        const labelFor = (element) => normalize([
          element.textContent,
          element.getAttribute('aria-label'),
          element.getAttribute('title'),
          element.value,
        ].filter(Boolean).join(' '));
        const isConsentOrTracking = (label) => /(?:cookie|privacy|tracking|consent|personal data)/i.test(label)
          && /(?:accept|agree|allow|reject|decline|settings|preferences|manage|continue|essential)/i.test(label);
        const authPattern = /\\b(?:log ?out|sign ?out|my account|account home|account settings|profile|dashboard)\\b/i;
        const hasLoginForm = Boolean(document.querySelector(
          'input[type="password"], input[name*="password" i], input[id*="password" i]'
        ));
        const controls = Array.from(document.querySelectorAll(
          'a, button, [role="button"], [role="link"], [role="menu"], [role="menuitem"], input[type="button"], input[type="submit"]'
        ));
        const hasAuthenticatedControl = controls.some((element) => {
          const label = labelFor(element);
          if (!label || isConsentOrTracking(label)) return false;
          const href = normalize(element.getAttribute('href'));
          return authPattern.test(label)
            || /\\/(?:logout|sign_out|profile|account|dashboard)(?:[/?#]|$)/i.test(href);
        });
        const bodyText = normalize(document.body?.innerText);
        const hasAuthenticatedText = !hasLoginForm && authPattern.test(bodyText);
        return hasAuthenticatedControl || hasAuthenticatedText;
      })()
    `, true);

    const inspectCookies = async () => {
      if (completed || authSession.popup !== popup || authContents.isDestroyed()) return;
      try {
        if (site.unauthenticatedUrlPattern?.test(authContents.getURL())) {
          emitAuthSessionState({
            status: 'active',
            current_url: authContents.getURL(),
            message: `Log in with ${site.label} to continue.`,
          });
          return;
        }
        const stored = await authContents.session.cookies.get({ url: site.cookieUrl });
        const values = new Map(stored.map((cookie) => [cookie.name, cookie.value]));
        const authenticatedCookieNames = site.authenticatedCookieNames ?? [];
        const missingAuthenticatedCookies = authenticatedCookieNames.filter((name) => !(values.get(name) || '').trim());
        if (authenticatedCookieNames.length > 0 && missingAuthenticatedCookies.length > 0) {
          emitAuthSessionState({
            status: 'active',
            current_url: authContents.getURL(),
            message: `Log in with ${site.label} to continue.`,
          });
          return;
        }

        if (authenticatedCookieNames.length === 0) {
          let hasAuthenticatedSignal = false;
          try {
            hasAuthenticatedSignal = await hasAuthenticatedDomSignal();
          } catch {
            return;
          }
          if (!hasAuthenticatedSignal) {
            emitAuthSessionState({
              status: 'active',
              current_url: authContents.getURL(),
              message: `Log in with ${site.label} to continue.`,
            });
            return;
          }
        }

        const cookieNames = site.cookieNames ?? [...values.keys()];
        const cookies = Object.fromEntries(
          cookieNames
            .map((name) => [name, (values.get(name) || '').trim()])
            .filter(([, value]) => value),
        );
        const missing = (site.cookieNames ?? []).filter((name) => !cookies[name]);
        if (missing.length > 0 || Object.keys(cookies).length === 0) {
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

    popup.on('closed', () => {
      if (inspectionTimer != null) clearInterval(inspectionTimer);
      inspectionTimer = null;
    });

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
    inspectionTimer = setInterval(() => { void inspectCookies(); }, 750);
    emitAuthSessionState({
      status: 'active',
      current_url: site.loginUrl,
      message: `Log in with ${site.label} in the popup window. Picto stores only the session cookies gallery-dl requires.`,
    });
    popup.focus();
    return authSession.state;
  }

  async function startOAuthSession(site) {
    const popup = authSession.popup;
    if (!popup || popup.isDestroyed()) throw new Error('Auth popup is unavailable.');
    const authContents = popup.webContents;
    const state = randomUUID();
    let authorizationUrl;
    let requestToken;
    let requestTokenSecret;
    let completed = false;
    let exchanging = false;

    if (site.id === 'baraag') {
      const registrationResponse = await fetchImpl(site.registerUrl, {
        method: 'POST',
        headers: { accept: 'application/json', 'content-type': 'application/json' },
        body: JSON.stringify({
          client_name: 'Picto',
          redirect_uris: OAUTH_CALLBACK_URL,
          scopes: 'read',
        }),
      });
      const registration = await responseJson(registrationResponse, 'Baraag application registration');
      if (!registration.client_id || !registration.client_secret) {
        throw new Error('Baraag application registration returned no client credentials.');
      }
      const params = new URLSearchParams({
        client_id: registration.client_id,
        redirect_uri: OAUTH_CALLBACK_URL,
        response_type: 'code',
        scope: 'read',
        state,
      });
      authorizationUrl = `${site.authorizeUrl}?${params}`;
      site = { ...site, clientId: registration.client_id, clientSecret: registration.client_secret };
    } else {
      const requestParams = { oauth_callback: OAUTH_CALLBACK_URL };
      const requestTokenResponse = await fetchImpl(site.requestTokenUrl, {
        method: 'POST',
        headers: {
          accept: 'application/x-www-form-urlencoded',
          Authorization: oauth1AuthorizationHeader({
            method: 'POST',
            url: site.requestTokenUrl,
            params: requestParams,
            consumerKey: site.consumerKey,
            consumerSecret: site.consumerSecret,
          }),
          'content-type': 'application/x-www-form-urlencoded',
        },
        body: formBody(requestParams),
      });
      const requestTokenData = await responseForm(requestTokenResponse, 'Tumblr request token');
      requestToken = requestTokenData.oauth_token;
      requestTokenSecret = requestTokenData.oauth_token_secret;
      if (!requestToken || !requestTokenSecret) {
        throw new Error('Tumblr request token response was incomplete.');
      }
      authorizationUrl = `${site.authorizeUrl}?${formBody({ oauth_token: requestToken, perms: 'read' })}`;
    }

    const callbackUrl = new URL(OAUTH_CALLBACK_URL);
    const isCallback = (url) => {
      try {
        const parsed = new URL(url);
        return parsed.origin === callbackUrl.origin && parsed.pathname === callbackUrl.pathname;
      } catch {
        return false;
      }
    };

    const handleCallback = async (url) => {
      if (completed || exchanging) return;
      exchanging = true;
      try {
        const params = new URL(url).searchParams;
        if (site.id === 'baraag') {
          if (params.get('state') !== state) throw new Error('Baraag OAuth state did not match.');
          const code = params.get('code');
          if (!code) throw new Error('No code in Baraag OAuth callback.');
          emitAuthSessionState({ status: 'loading', current_url: url, message: 'Completing Baraag authorization…' });
          const tokenResponse = await fetchImpl(site.tokenUrl, {
            method: 'POST',
            headers: { accept: 'application/json', 'content-type': 'application/x-www-form-urlencoded' },
            body: formBody({
              client_id: site.clientId,
              client_secret: site.clientSecret,
              grant_type: 'authorization_code',
              code,
              redirect_uri: OAUTH_CALLBACK_URL,
            }),
          });
          const token = await responseJson(tokenResponse, 'Baraag token exchange');
          if (!token.access_token) throw new Error('Baraag token response was incomplete.');
          completed = true;
          await completeAuthSession({
            site_category: 'baraag',
            credential_type: 'oauth_token',
            oauth_token: token.access_token,
          }, 'Baraag authorization completed.');
        } else {
          if (params.get('oauth_token') !== requestToken) throw new Error('Tumblr OAuth token did not match.');
          const verifier = params.get('oauth_verifier');
          if (!verifier) throw new Error('No verifier in Tumblr OAuth callback.');
          emitAuthSessionState({ status: 'loading', current_url: url, message: 'Completing Tumblr authorization…' });
          const exchangeParams = { oauth_token: requestToken, oauth_verifier: verifier };
          const tokenResponse = await fetchImpl(site.tokenUrl, {
            method: 'POST',
            headers: {
              accept: 'application/x-www-form-urlencoded',
              Authorization: oauth1AuthorizationHeader({
                method: 'POST',
                url: site.tokenUrl,
                params: exchangeParams,
                consumerKey: site.consumerKey,
                consumerSecret: site.consumerSecret,
                token: requestToken,
                tokenSecret: requestTokenSecret,
              }),
              'content-type': 'application/x-www-form-urlencoded',
            },
            body: formBody(exchangeParams),
          });
          const token = await responseForm(tokenResponse, 'Tumblr access token exchange');
          if (!token.oauth_token || !token.oauth_token_secret) {
            throw new Error('Tumblr access token response was incomplete.');
          }
          completed = true;
          await completeAuthSession({
            site_category: 'tumblr',
            credential_type: 'oauth_token',
            oauth_token: token.oauth_token,
            password: token.oauth_token_secret,
          }, 'Tumblr authorization completed.');
        }
      } catch (err) {
        exchanging = false;
        try {
          if (!popup.isDestroyed()) popup.show();
        } catch {}
        emitAuthSessionState({
          status: 'error',
          current_url: url,
          message: err instanceof Error ? err.message : 'OAuth authorization failed.',
          credential: null,
        });
      }
    };

    const interceptCallback = (event, url) => {
      if (!isCallback(url)) return;
      event.preventDefault();
      void handleCallback(url);
    };
    authContents.on('will-redirect', interceptCallback);
    authContents.on('will-navigate', interceptCallback);
    authContents.on('page-title-updated', (_event, title) => {
      emitAuthSessionState({ title });
    });
    authContents.on('did-fail-load', (_event, errorCode, errorDescription, validatedUrl, isMainFrame) => {
      if (isMainFrame === false || errorCode === -3) return;
      emitAuthSessionState({
        status: 'error',
        current_url: validatedUrl || authContents.getURL(),
        message: `${site.label} could not load in the login window (${errorDescription || `error ${errorCode}`}).`,
        credential: null,
      });
    });

    await authContents.loadURL(authorizationUrl);
    emitAuthSessionState({
      status: 'active',
      current_url: authorizationUrl,
      message: `Authorize Picto with ${site.label} in the popup window.`,
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

    const siteConfig = BOORU_AUTH_SITES[site] ?? OAUTH_AUTH_SITES[site] ?? COOKIE_AUTH_SITES[site];
    const title = site === 'pixiv'
      ? 'Pixiv Login'
      : `Login: ${siteConfig?.label ?? site}`;
    const popup = createAuthSessionWindow(site, title);
    const authContents = popup.webContents;
    emitAuthSessionState({
      site_category: site,
      status: 'starting',
      title,
      current_url: startUrl,
      message: BOORU_AUTH_SITES[site]
        ? `Log in in the popup window, then Picto will read the ${BOORU_AUTH_SITES[site].label} account options page for user_id and api_key.`
        : OAUTH_AUTH_SITES[site]
          ? `Authorize Picto with ${OAUTH_AUTH_SITES[site].label} in the popup window.`
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

    if (OAUTH_AUTH_SITES[site]) {
      try {
        return await startOAuthSession(OAUTH_AUTH_SITES[site]);
      } catch (err) {
        emitAuthSessionState({
          status: 'error',
          current_url: authContents.getURL(),
          message: err instanceof Error ? err.message : `${siteConfig.label} OAuth setup failed.`,
          credential: null,
        });
        return authSession.state;
      }
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

  function getAuthSessionState() {
    return authSession.state;
  }

  return {
    startAuthSession,
    cancelAuthSession,
    getAuthSessionState,
  };
}
