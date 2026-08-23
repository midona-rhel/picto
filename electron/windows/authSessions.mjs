import { createHmac, randomUUID } from 'node:crypto';
import { getStaticAuthLoginRoutes, resolveAuthSite } from './authSites.mjs';

const OAUTH_CALLBACK_URL = 'https://picto.app/oauth/callback';
const POLL_INTERVAL_MS = 750;

export { getStaticAuthLoginRoutes };

function formBody(values) {
  return new URLSearchParams(values).toString();
}

function sanitizeUserAgent(userAgent) {
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

function oauth1AuthorizationHeader({ method, url, params, site, token, tokenSecret }) {
  const oauthParams = {
    oauth_consumer_key: site.consumerKey,
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
  const base = [method.toUpperCase(), url, signatureParams].map(oauthEncode).join('&');
  const key = `${oauthEncode(site.consumerSecret)}&${oauthEncode(tokenSecret || '')}`;
  const signature = createHmac('sha1', key).update(base).digest('base64');
  return `OAuth ${Object.entries({ ...oauthParams, oauth_signature: signature })
    .map(([name, value]) => `${oauthEncode(name)}="${oauthEncode(value)}"`)
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
    throw new Error(`${operation} failed: ${body?.error_description || body?.error || response.status}`);
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

async function inspectAccountApiPage(webContents) {
  return webContents.executeJavaScript(String.raw`
    (() => {
      const text = document.body?.innerText || '';
      const href = location.href;
      const consent = /agree to the usage of cookies according to our cookie policy/i.test(text)
        ? Array.from(document.querySelectorAll('button, a, [role="button"], input[type="button"], input[type="submit"]'))
            .find((element) => ((element.textContent || element.value || '').trim().toLowerCase() === 'accept'))
        : null;
      if (consent) consent.click();
      const inputs = Array.from(document.querySelectorAll('input, textarea'));
      const readInput = (...needles) => {
        for (const input of inputs) {
          const key = (input.name || input.id || '').toLowerCase();
          const value = (input.value || '').trim();
          if (value && needles.some((needle) => key.includes(needle))) return value;
        }
        return null;
      };
      const snippets = [
        text,
        href,
        ...inputs.map((input) => (input.value || '').trim()),
        ...Array.from(document.querySelectorAll('a[href]')).map((anchor) => anchor.getAttribute('href') || ''),
      ];
      const combined = snippets.map((value) => {
        const apiKey = value.match(/api[_-]?key=([a-f0-9]{16,})/i)?.[1] || null;
        const userId = value.match(/user[_-]?id=(\d+)/i)?.[1] || null;
        return apiKey ? { apiKey, userId } : null;
      }).find(Boolean);
      const apiKeyInput = readInput('api_key', 'api-key');
      const apiKey = /^[a-f0-9]{16,}$/i.test(apiKeyInput || '')
        ? apiKeyInput
        : text.match(/api[_ -]?key\s*[:\n]\s*([a-f0-9]{16,})/i)?.[1] || combined?.apiKey || null;
      const userId = readInput('user_id', 'user-id')
        || text.match(/user[_ -]?id\s*[:\n]\s*(\d+)/i)?.[1]
        || combined?.userId
        || null;
      const labels = Array.from(document.querySelectorAll('a')).map((anchor) =>
        (anchor.textContent || '').trim().toLowerCase().replace(/[^a-z]+/g, ' ').trim()
      );
      const hasChallenge = Boolean(document.querySelector('#challenge-running, #challenge-stage, .cf-challenge'))
        || /just a moment|verify (?:that )?you are human|checking your browser|cloudflare/i.test((document.title || '') + '\n' + text);
      return {
        href,
        title: document.title || null,
        acceptedConsent: Boolean(consent),
        hasChallenge,
        hasLoginForm: Boolean(document.querySelector('input[name="user"], input[name="username"], input[type="password"]')),
        authenticated: labels.some((label) => label === 'logout' || label.endsWith(' logout'))
          || /account home/i.test(text),
        onOptions: /account options/i.test(text) || /[?&]s=options(?:&|$)/i.test(href),
        apiKey,
        userId,
      };
    })()
  `, true);
}

async function hasAuthenticatedDomSignal(webContents) {
  return webContents.executeJavaScript(String.raw`
    (() => {
      const normalize = (value) => (value || '').replace(/\s+/g, ' ').trim().toLowerCase();
      const hasLoginForm = Boolean(document.querySelector('input[type="password"], input[name*="password" i]'));
      const controls = Array.from(document.querySelectorAll('a, button, [role="button"], [role="link"], [role="menuitem"]'));
      const authPattern = /\b(?:log ?out|sign ?out|my account|account home|account settings|profile|dashboard)\b/i;
      return controls.some((element) => {
        const label = normalize([element.textContent, element.getAttribute('aria-label'), element.getAttribute('title')].filter(Boolean).join(' '));
        const href = normalize(element.getAttribute('href'));
        const consent = /(?:cookie|privacy|tracking|consent)/i.test(label)
          && /(?:accept|agree|allow|reject|settings|preferences)/i.test(label);
        return !consent && (authPattern.test(label) || /\/(?:logout|sign_out|profile|account|dashboard)(?:[/?#]|$)/i.test(href));
      }) || (!hasLoginForm && authPattern.test(normalize(document.body?.innerText)));
    })()
  `, true);
}

function createCookieAdapter(site) {
  return {
    async prepare() {
      return { url: site.loginUrl, message: `Log in with ${site.label} in the popup window.` };
    },
    async inspect(contents) {
      const currentUrl = contents.getURL();
      if (site.unauthenticatedUrlPattern?.test(currentUrl)) {
        return { status: 'active', message: `Log in with ${site.label} to continue.` };
      }
      const stored = await contents.session.cookies.get({ url: site.cookieUrl });
      const values = new Map(stored.map((cookie) => [cookie.name, cookie.value]));
      const authenticatedNames = site.authenticatedCookieNames ?? [];
      if (authenticatedNames.some((name) => !(values.get(name) || '').trim())) {
        return { status: 'active', message: `Log in with ${site.label} to continue.` };
      }
      if (authenticatedNames.length === 0 && !(await hasAuthenticatedDomSignal(contents))) {
        return { status: 'active', message: `Log in with ${site.label} to continue.` };
      }
      const names = site.cookieNames ?? [...values.keys()];
      const cookies = Object.fromEntries(names
        .map((name) => [name, (values.get(name) || '').trim()])
        .filter(([, value]) => value));
      if ((site.cookieNames ?? []).some((name) => !cookies[name]) || Object.keys(cookies).length === 0) {
        return { status: 'active', message: `Log in with ${site.label} to continue.` };
      }
      return {
        credential: { site_category: site.id, credential_type: 'cookies', cookies },
        message: `${site.label} session captured.`,
      };
    },
  };
}

function createAccountApiAdapter(site) {
  let navigatingToOptions = false;
  return {
    async prepare() {
      return { url: site.loginUrl, message: `Log in with ${site.label} in the popup window.` };
    },
    async inspect(contents) {
      const result = await inspectAccountApiPage(contents);
      if (result.acceptedConsent) return { status: 'active', message: `Preparing ${site.label} login…` };
      if (result.hasChallenge) {
        return { status: 'active', message: `Complete the ${site.label} browser check; Picto will continue automatically.` };
      }
      if (result.apiKey && result.userId) {
        return {
          credential: {
            site_category: site.id,
            credential_type: 'api_key',
            username: result.userId,
            password: result.apiKey,
          },
          message: `${site.label} API key captured.`,
        };
      }
      if (result.authenticated && !result.onOptions && !navigatingToOptions) {
        navigatingToOptions = true;
        return {
          navigate: site.optionsUrl,
          hide: true,
          status: 'loading',
          message: `Authenticated. Reading ${site.label} API credentials…`,
        };
      }
      if (result.onOptions && navigatingToOptions) {
        return {
          show: true,
          status: 'error',
          message: `Logged in, but Picto could not find ${site.label} user_id and api_key.`,
        };
      }
      return {
        status: 'active',
        message: result.hasLoginForm ? `Log in with ${site.label} to continue.` : `Waiting for ${site.label} login…`,
      };
    },
  };
}

function createOAuthAdapter(site, fetchImpl) {
  let authorizationUrl;
  let requestToken;
  let requestTokenSecret;
  let oauthState;
  let clientId;
  let clientSecret;
  return {
    async prepare() {
      if (site.strategy === 'oauth2') {
        oauthState = randomUUID();
        const registration = await responseJson(await fetchImpl(site.registerUrl, {
          method: 'POST',
          headers: { accept: 'application/json', 'content-type': 'application/json' },
          body: JSON.stringify({ client_name: 'Picto', redirect_uris: OAUTH_CALLBACK_URL, scopes: 'read' }),
        }), `${site.label} application registration`);
        clientId = registration.client_id;
        clientSecret = registration.client_secret;
        if (!clientId || !clientSecret) throw new Error(`${site.label} application registration returned no client credentials.`);
        authorizationUrl = `${site.authorizeUrl}?${formBody({
          client_id: clientId,
          redirect_uri: OAUTH_CALLBACK_URL,
          response_type: 'code',
          scope: 'read',
          state: oauthState,
        })}`;
      } else {
        const params = { oauth_callback: OAUTH_CALLBACK_URL };
        const token = await responseForm(await fetchImpl(site.requestTokenUrl, {
          method: 'POST',
          headers: {
            accept: 'application/x-www-form-urlencoded',
            Authorization: oauth1AuthorizationHeader({ method: 'POST', url: site.requestTokenUrl, params, site }),
            'content-type': 'application/x-www-form-urlencoded',
          },
          body: formBody(params),
        }), `${site.label} request token`);
        requestToken = token.oauth_token;
        requestTokenSecret = token.oauth_token_secret;
        if (!requestToken || !requestTokenSecret) throw new Error(`${site.label} request token response was incomplete.`);
        authorizationUrl = `${site.authorizeUrl}?${formBody({ oauth_token: requestToken, perms: 'read' })}`;
      }
      return { url: authorizationUrl, message: `Authorize Picto with ${site.label} in the popup window.` };
    },
    handles(url) {
      try {
        const parsed = new URL(url);
        const callback = new URL(OAUTH_CALLBACK_URL);
        return parsed.origin === callback.origin && parsed.pathname === callback.pathname;
      } catch {
        return false;
      }
    },
    async complete(url) {
      const params = new URL(url).searchParams;
      if (site.strategy === 'oauth2') {
        if (params.get('state') !== oauthState) throw new Error(`${site.label} OAuth state did not match.`);
        const code = params.get('code');
        if (!code) throw new Error(`No code in ${site.label} OAuth callback.`);
        const token = await responseJson(await fetchImpl(site.tokenUrl, {
          method: 'POST',
          headers: { accept: 'application/json', 'content-type': 'application/x-www-form-urlencoded' },
          body: formBody({ client_id: clientId, client_secret: clientSecret, grant_type: 'authorization_code', code, redirect_uri: OAUTH_CALLBACK_URL }),
        }), `${site.label} token exchange`);
        if (!token.access_token) throw new Error(`${site.label} token response was incomplete.`);
        return {
          credential: { site_category: site.id, credential_type: 'oauth_token', oauth_token: token.access_token },
          message: `${site.label} authorization completed.`,
        };
      }
      if (params.get('oauth_token') !== requestToken) throw new Error(`${site.label} OAuth token did not match.`);
      const verifier = params.get('oauth_verifier');
      if (!verifier) throw new Error(`No verifier in ${site.label} OAuth callback.`);
      const values = { oauth_token: requestToken, oauth_verifier: verifier };
      const token = await responseForm(await fetchImpl(site.tokenUrl, {
        method: 'POST',
        headers: {
          accept: 'application/x-www-form-urlencoded',
          Authorization: oauth1AuthorizationHeader({
            method: 'POST', url: site.tokenUrl, params: values, site, token: requestToken, tokenSecret: requestTokenSecret,
          }),
          'content-type': 'application/x-www-form-urlencoded',
        },
        body: formBody(values),
      }), `${site.label} access token exchange`);
      if (!token.oauth_token || !token.oauth_token_secret) throw new Error(`${site.label} access token response was incomplete.`);
      return {
        credential: {
          site_category: site.id,
          credential_type: 'oauth_token',
          oauth_token: token.oauth_token,
          password: token.oauth_token_secret,
        },
        message: `${site.label} authorization completed.`,
      };
    },
  };
}

function createPixivAdapter(site, beginPixivOAuth, completePixivOAuth) {
  let codeVerifier;
  return {
    async prepare() {
      const challenge = await beginPixivOAuth();
      codeVerifier = challenge?.code_verifier;
      if (!challenge?.login_url || !codeVerifier) throw new Error('Pixiv OAuth challenge was incomplete.');
      return { url: challenge.login_url, message: 'Sign in with Pixiv in the popup window.' };
    },
    handles(url) {
      return String(url || '').startsWith('pixiv://');
    },
    async complete(url, contents) {
      const code = new URL(url).searchParams.get('code');
      if (!code) throw new Error('No code in Pixiv callback.');
      let phpsessid = null;
      try {
        phpsessid = (await contents.session.cookies.get({ domain: '.pixiv.net', name: 'PHPSESSID' }))[0]?.value ?? null;
      } catch {}
      await completePixivOAuth({ code, code_verifier: codeVerifier, phpsessid });
      return { alreadyPersisted: true, message: 'Pixiv authorization completed.' };
    },
  };
}

function createAdapter(site, dependencies) {
  if (site.strategy === 'cookies') return createCookieAdapter(site);
  if (site.strategy === 'account_api') return createAccountApiAdapter(site);
  if (site.strategy === 'oauth1' || site.strategy === 'oauth2') return createOAuthAdapter(site, dependencies.fetchImpl);
  if (site.strategy === 'pixiv') return createPixivAdapter(site, dependencies.beginPixivOAuth, dependencies.completePixivOAuth);
  throw new Error(`Unsupported authentication strategy: ${site.strategy}`);
}

export function createAuthSessions({
  BrowserWindow,
  getMainWindow,
  fetchImpl = fetch,
  persistCredential = async () => { throw new Error('Credential persistence is unavailable.'); },
  beginPixivOAuth = async () => { throw new Error('Pixiv OAuth is unavailable.'); },
  completePixivOAuth = async () => { throw new Error('Pixiv OAuth is unavailable.'); },
}) {
  let popup = null;
  let adapter = null;
  let pollTimer = null;
  let closing = false;
  let inspecting = false;
  let finishing = false;
  let completed = false;
  let state = {
    site_category: null,
    status: 'idle',
    title: null,
    current_url: null,
    message: null,
  };

  function emit(patch) {
    state = { ...state, ...patch };
    for (const win of BrowserWindow.getAllWindows()) {
      if (!win.isDestroyed()) win.webContents.send('auth:session-state', state);
    }
    return state;
  }

  async function closePopup({ clearStorage = false } = {}) {
    if (pollTimer != null) clearInterval(pollTimer);
    pollTimer = null;
    const current = popup;
    popup = null;
    adapter = null;
    inspecting = false;
    if (!current || current.isDestroyed()) return;
    if (clearStorage) {
      try { await current.webContents.session.clearStorageData(); } catch {}
    }
    closing = true;
    try { current.close(); } catch {}
    closing = false;
  }

  async function finish(result) {
    if (completed || finishing) return;
    finishing = true;
    emit({ status: 'loading', message: 'Saving login securely…' });
    try {
      if (!result.alreadyPersisted) {
        const credential = result.credential;
        await persistCredential({
          site_id: credential.site_category,
          credential_type: credential.credential_type,
          display_name: credential.display_name ?? credential.site_category,
          username: credential.username ?? null,
          password: credential.password ?? null,
          cookies: credential.cookies ?? null,
          oauth_token: credential.oauth_token ?? null,
        });
      }
      completed = true;
      emit({ status: 'completed', message: result.message || 'Login completed.' });
      await closePopup({ clearStorage: true });
    } finally {
      finishing = false;
    }
  }

  async function applyResult(result) {
    if (!result || completed || !popup) return;
    if (result.hide) popup.hide();
    if (result.show) popup.show();
    if (result.credential || result.alreadyPersisted) {
      await finish(result);
      return;
    }
    emit({
      ...(result.status ? { status: result.status } : {}),
      ...(result.message ? { message: result.message } : {}),
    });
    if (result.navigate) await popup.webContents.loadURL(result.navigate);
  }

  async function inspect() {
    if (inspecting || completed || !popup || popup.isDestroyed() || !adapter?.inspect) return;
    inspecting = true;
    try {
      await applyResult(await adapter.inspect(popup.webContents));
    } catch (error) {
      emit({ status: 'error', message: error instanceof Error ? error.message : 'Failed to inspect login state.' });
    } finally {
      inspecting = false;
    }
  }

  function createPopup(site) {
    const parent = getMainWindow();
    const authWindow = new BrowserWindow({
      width: 520,
      height: 760,
      minWidth: 420,
      minHeight: 640,
      title: `Login: ${site.label}`,
      backgroundColor: '#ffffff',
      autoHideMenuBar: true,
      show: false,
      ...(parent && !parent.isDestroyed() ? { parent } : {}),
      webPreferences: {
        contextIsolation: true,
        nodeIntegration: false,
        sandbox: true,
        partition: `persist:picto-auth-v1-${site.id}`,
      },
    });
    const userAgent = sanitizeUserAgent(authWindow.webContents.getUserAgent?.());
    if (userAgent) authWindow.webContents.setUserAgent(userAgent);
    authWindow.webContents.setWindowOpenHandler(({ url }) => {
      if (/^https:\/\//i.test(url)) queueMicrotask(() => {
        if (!authWindow.isDestroyed()) void authWindow.webContents.loadURL(url);
      });
      return { action: 'deny' };
    });
    authWindow.once('ready-to-show', () => { if (!authWindow.isDestroyed()) authWindow.show(); });
    authWindow.on('closed', () => {
      const cancelled = !closing && popup === authWindow;
      if (pollTimer != null) clearInterval(pollTimer);
      pollTimer = null;
      if (popup === authWindow) popup = null;
      if (cancelled) emit({
        site_category: null,
        status: 'cancelled',
        title: null,
        current_url: null,
        message: 'Login window closed.',
      });
    });
    popup = authWindow;
    return authWindow;
  }

  function bindWindow(site, authWindow) {
    const contents = authWindow.webContents;
    contents.on('page-title-updated', (_event, title) => emit({ title }));
    contents.on('did-navigate', (_event, url) => {
      emit({ status: 'loading', current_url: url });
      void inspect();
    });
    contents.on('did-navigate-in-page', (_event, url) => {
      emit({ status: 'loading', current_url: url });
      void inspect();
    });
    contents.on('did-finish-load', () => {
      emit({ status: 'active', current_url: contents.getURL() });
      void inspect();
    });
    contents.on('did-fail-load', (_event, code, description, url, isMainFrame) => {
      if (isMainFrame === false || code === -3) return;
      authWindow.show();
      emit({
        status: 'error',
        current_url: url || contents.getURL(),
        message: `${site.label} could not load (${description || `error ${code}`}).`,
      });
    });
    const intercept = (event, url) => {
      if (!adapter?.handles?.(url)) return;
      event.preventDefault();
      emit({ status: 'loading', current_url: url, message: `Completing ${site.label} login…` });
      void adapter.complete(url, contents).then(applyResult).catch((error) => {
        authWindow.show();
        emit({ status: 'error', message: error instanceof Error ? error.message : `${site.label} login failed.` });
      });
    };
    contents.on('will-redirect', intercept);
    contents.on('will-navigate', intercept);
  }

  async function startAuthSession(siteCategory) {
    await cancelAuthSession();
    const site = resolveAuthSite(siteCategory);
    if (!site) throw new Error(`Unsupported auth site: ${String(siteCategory || '').trim().toLowerCase()}`);
    completed = false;
    finishing = false;
    inspecting = false;
    adapter = createAdapter(site, { fetchImpl, beginPixivOAuth, completePixivOAuth });
    const authWindow = createPopup(site);
    bindWindow(site, authWindow);
    emit({
      site_category: site.id,
      status: 'starting',
      title: `Login: ${site.label}`,
      current_url: null,
      message: 'Preparing login…',
    });
    try {
      const prepared = await adapter.prepare();
      await authWindow.webContents.loadURL(prepared.url);
      emit({ status: 'active', current_url: prepared.url, message: prepared.message });
      if (adapter.inspect) pollTimer = setInterval(() => { void inspect(); }, POLL_INTERVAL_MS);
      authWindow.focus();
    } catch (error) {
      authWindow.show();
      emit({ status: 'error', message: error instanceof Error ? error.message : `${site.label} login setup failed.` });
    }
    return state;
  }

  async function cancelAuthSession() {
    await closePopup();
    completed = false;
    finishing = false;
    emit({
      site_category: null,
      status: 'idle',
      title: null,
      current_url: null,
      message: null,
    });
  }

  return {
    startAuthSession,
    cancelAuthSession,
    getAuthSessionState: () => state,
  };
}
