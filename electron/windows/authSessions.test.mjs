import { describe, expect, it, vi } from 'vitest';
import { createAuthSessions, getStaticAuthLoginRoutes } from './authSessions.mjs';
import { resolveAuthSite } from './authSites.mjs';

const settle = async () => {
  await new Promise((resolve) => setTimeout(resolve, 0));
  await new Promise((resolve) => setTimeout(resolve, 0));
};

function createBrowserWindowMock({ pageResult = false, cookies = [] } = {}) {
  const instances = [];

  class FakeBrowserWindow {
    constructor(options) {
      this.options = options;
      this.listeners = new Map();
      this.messages = [];
      this.hideCalls = 0;
      this.showCalls = 0;
      this.destroyed = false;
      this.webContents = {
        listeners: new Map(),
        loadURL: vi.fn(async (url) => { this.loadedUrl = url; }),
        on: (event, handler) => { this.webContents.listeners.set(event, handler); },
        send: (_channel, payload) => { this.messages.push(payload); },
        getUserAgent: () => 'Mozilla/5.0 Electron/37.0.0 Picto/0.5.0',
        setUserAgent: (value) => { this.userAgent = value; },
        setWindowOpenHandler: (handler) => { this.windowOpenHandler = handler; },
        getURL: () => this.loadedUrl ?? '',
        isDestroyed: () => this.destroyed,
        executeJavaScript: vi.fn(async (script) => {
          new Function(script);
          return typeof pageResult === 'function' ? pageResult(this.loadedUrl, script) : pageResult;
        }),
        session: {
          clearCache: vi.fn(async () => {}),
          clearStorageData: vi.fn(async () => {}),
          setPermissionCheckHandler: vi.fn((handler) => { this.permissionCheckHandler = handler; }),
          setPermissionRequestHandler: vi.fn((handler) => { this.permissionRequestHandler = handler; }),
          webRequest: {
            onBeforeSendHeaders: vi.fn((filter, handler) => {
              this.beforeSendHeaders = filter ? handler : null;
            }),
          },
          cookies: {
            get: vi.fn(async () => (typeof cookies === 'function' ? cookies() : cookies)),
          },
        },
      };
      instances.push(this);
    }

    once(event, handler) { this.listeners.set(event, handler); }
    on(event, handler) { this.listeners.set(event, handler); }
    isDestroyed() { return this.destroyed; }
    close() { this.destroyed = true; this.listeners.get('closed')?.(); }
    focus() {}
    hide() { this.hideCalls += 1; }
    show() { this.showCalls += 1; }
  }

  FakeBrowserWindow.getAllWindows = () => instances;
  return { BrowserWindow: FakeBrowserWindow, instances };
}

function createHarness(browser, overrides = {}) {
  const persistCredential = vi.fn(async () => ({ revision: 1 }));
  const beginPixivOAuth = vi.fn(async () => ({
    login_url: 'https://app-api.pixiv.net/web/v1/login',
    code_verifier: 'verifier',
  }));
  const completePixivOAuth = vi.fn(async () => ({ ok: true }));
  const sessions = createAuthSessions({
    BrowserWindow: browser.BrowserWindow,
    getMainWindow: () => null,
    persistCredential,
    beginPixivOAuth,
    completePixivOAuth,
    launchOnlyFansAuth: null,
    ...overrides,
  });
  return { sessions, persistCredential, beginPixivOAuth, completePixivOAuth };
}

describe('direct-site authentication', () => {
  it('opens Twitter / X at its direct-site login and captures its session cookies', () => {
    expect(resolveAuthSite('twitter')).toMatchObject({
      loginUrl: 'https://x.com/i/flow/login',
      cookieUrl: 'https://x.com',
      authenticatedCookieNames: ['auth_token', 'ct0'],
    });
  });

  it('captures SubscribeStar cookies from the same host used by its downloader', () => {
    expect(resolveAuthSite('subscribestar')).toMatchObject({
      loginUrl: 'https://subscribestar.art/login',
      cookieUrl: 'https://subscribestar.art',
    });
  });

  it('captures the Newgrounds session used by gallery-dl', () => {
    expect(resolveAuthSite('newgrounds')).toMatchObject({
      loginUrl: 'https://www.newgrounds.com/login',
      cookieUrl: 'https://www.newgrounds.com',
      cookieNames: ['ng_session'],
      authenticatedCookieNames: ['ng_session'],
      authWindowSize: { width: 1000, height: 760, minWidth: 760, minHeight: 640 },
    });
  });

  it('uses the E-Hentai account login and a separate ExHentai verification step', () => {
    expect(resolveAuthSite('exhentai')).toMatchObject({
      loginUrl: 'https://forums.e-hentai.org/index.php?act=Login&CODE=00',
      verificationUrl: 'https://exhentai.org/',
      cookieNames: ['ipb_member_id', 'ipb_pass_hash', 'igneous'],
      authenticatedCookieNames: ['ipb_member_id', 'ipb_pass_hash'],
      resetSessionOnStart: true,
    });
  });

  it('verifies ExHentai access before persisting its gallery-dl cookies', async () => {
    const browser = createBrowserWindowMock({
      pageResult: (url) => url.includes('exhentai.org')
        ? { href: url, host: 'exhentai.org', hasLoginForm: false, accessDenied: false, blank: false }
        : { href: url, host: 'forums.e-hentai.org', hasLoginForm: false, accessDenied: false, blank: false },
      cookies: [
        { name: 'ipb_member_id', value: 'member' },
        { name: 'ipb_pass_hash', value: 'hash' },
        { name: 'igneous', value: 'igneous' },
        { name: 'analytics', value: 'ignored' },
      ],
    });
    const { sessions, persistCredential } = createHarness(browser);

    await sessions.startAuthSession('exhentai');
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await settle();
    expect(browser.instances[0].loadedUrl).toBe('https://exhentai.org/');
    expect(browser.instances[0].hideCalls).toBe(1);

    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await settle();
    expect(persistCredential).toHaveBeenCalledWith(expect.objectContaining({
      site_id: 'exhentai',
      credential_type: 'cookies',
      cookies: {
        ipb_member_id: 'member',
        ipb_pass_hash: 'hash',
        igneous: 'igneous',
      },
    }));
    expect(sessions.getAuthSessionState().status).toBe('completed');
  });

  it('rejects an expired ExHentai session when verification returns Sad Panda', async () => {
    const browser = createBrowserWindowMock({
      pageResult: (url) => url.includes('exhentai.org')
        ? { href: url, host: 'exhentai.org', hasLoginForm: false, accessDenied: true, blank: false }
        : { href: url, host: 'forums.e-hentai.org', hasLoginForm: false, accessDenied: false, blank: false },
      cookies: [
        { name: 'ipb_member_id', value: 'expired-member' },
        { name: 'ipb_pass_hash', value: 'expired-hash' },
      ],
    });
    const { sessions, persistCredential } = createHarness(browser);

    await sessions.startAuthSession('exhentai');
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await settle();
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await settle();

    expect(persistCredential).not.toHaveBeenCalled();
    expect(browser.instances[0].loadedUrl).toBe('https://forums.e-hentai.org/index.php?act=Login&CODE=00');
    expect(browser.instances[0].webContents.session.clearStorageData).toHaveBeenCalledTimes(2);
    expect(browser.instances[0].showCalls).toBeGreaterThan(0);
    expect(sessions.getAuthSessionState()).toMatchObject({
      status: 'active',
      message: expect.stringContaining('Sad Panda'),
    });
  });

  it('opens Newgrounds at desktop width without changing other login windows', async () => {
    const newgroundsBrowser = createBrowserWindowMock();
    const { sessions: newgroundsSessions } = createHarness(newgroundsBrowser);
    await newgroundsSessions.startAuthSession('newgrounds');
    expect(newgroundsBrowser.instances[0].options).toMatchObject({
      width: 1000,
      height: 760,
      minWidth: 760,
      minHeight: 640,
    });
    await newgroundsSessions.cancelAuthSession();

    const patreonBrowser = createBrowserWindowMock();
    const { sessions: patreonSessions } = createHarness(patreonBrowser);
    await patreonSessions.startAuthSession('patreon');
    expect(patreonBrowser.instances[0].options).toMatchObject({
      width: 520,
      height: 760,
      minWidth: 420,
      minHeight: 640,
    });
    await patreonSessions.cancelAuthSession();
  });

  it('rejects unsupported sites without creating a browser window', async () => {
    const browser = createBrowserWindowMock();
    const { sessions } = createHarness(browser);

    await expect(sessions.startAuthSession('missing')).rejects.toThrow('Unsupported auth site: missing');
    expect(browser.instances).toHaveLength(0);
  });

  it('opens every cookie and account source at its declared direct-site login route', async () => {
    const routes = getStaticAuthLoginRoutes().filter((route) =>
      ['cookies', 'account_api'].includes(resolveAuthSite(route.site)?.strategy)
    );
    for (const route of routes) {
      const browser = createBrowserWindowMock();
      const { sessions } = createHarness(browser);
      const state = await sessions.startAuthSession(route.site);

      expect(browser.instances[0].loadedUrl, route.site).toBe(route.loginUrl);
      expect(state, route.site).toMatchObject({ site_category: route.site, status: 'active' });
      await sessions.cancelAuthSession();
    }
  });

  it('persists cookie credentials in the host and never sends secrets to the renderer', async () => {
    const browser = createBrowserWindowMock({
      pageResult: true,
      cookies: [{ name: '_session', value: 'secret' }, { name: 'user', value: '42' }],
    });
    const { sessions, persistCredential } = createHarness(browser);

    await sessions.startAuthSession('danbooru');
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await settle();

    expect(persistCredential).toHaveBeenCalledWith({
      site_id: 'danbooru',
      credential_type: 'cookies',
      display_name: 'danbooru',
      username: null,
      password: null,
      cookies: { _session: 'secret', user: '42' },
      headers: null,
      oauth_token: null,
    });
    expect(browser.instances[0].messages.some((message) => Object.hasOwn(message, 'credential'))).toBe(false);
    expect(sessions.getAuthSessionState().status).toBe('completed');
  });

  it('captures only cookies required by a source', async () => {
    const browser = createBrowserWindowMock({
      cookies: [
        { name: 'a', value: 'required-a' },
        { name: 'b', value: 'required-b' },
        { name: 'analytics', value: 'ignored' },
      ],
    });
    const { sessions, persistCredential } = createHarness(browser);

    await sessions.startAuthSession('furaffinity');
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await settle();

    expect(persistCredential.mock.calls[0][0].cookies).toEqual({ a: 'required-a', b: 'required-b' });
  });

  it('opens Fur Affinity with a fresh unspoofed session and only storage access enabled', async () => {
    const browser = createBrowserWindowMock();
    const { sessions } = createHarness(browser);

    await sessions.startAuthSession('furaffinity');

    const popup = browser.instances[0];
    expect(popup.userAgent).toBeUndefined();
    expect(popup.webContents.session.clearCache).toHaveBeenCalledOnce();
    expect(popup.webContents.session.clearStorageData).toHaveBeenCalledOnce();
    expect(popup.permissionCheckHandler(null, 'storage-access')).toBe(true);
    expect(popup.permissionCheckHandler(null, 'top-level-storage-access')).toBe(true);
    expect(popup.permissionCheckHandler(null, 'media')).toBe(false);

    const storageCallback = vi.fn();
    popup.permissionRequestHandler(null, 'storage-access', storageCallback);
    expect(storageCallback).toHaveBeenCalledWith(true);
    const notificationCallback = vi.fn();
    popup.permissionRequestHandler(null, 'notifications', notificationCallback);
    expect(notificationCallback).toHaveBeenCalledWith(false);
  });

  it('captures the complete OnlyFans browser session from an authenticated API request', async () => {
    const browser = createBrowserWindowMock({
      cookies: [
        { name: 'sess', value: 'session' },
        { name: 'auth_id', value: '42' },
        { name: 'auth_uid', value: '42' },
        { name: 'analytics', value: 'ignored' },
      ],
    });
    const { sessions, persistCredential } = createHarness(browser);

    await sessions.startAuthSession('onlyfans');
    browser.instances[0].beforeSendHeaders({
      requestHeaders: { 'X-BC': 'browser-signature', 'User-Agent': 'OnlyFans browser' },
    }, vi.fn());
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await settle();

    expect(persistCredential).toHaveBeenCalledWith(expect.objectContaining({
      site_id: 'onlyfans',
      credential_type: 'cookies',
      cookies: { sess: 'session', auth_id: '42', auth_uid: '42' },
      headers: { 'x-bc': 'browser-signature', 'user-agent': 'OnlyFans browser' },
    }));
    expect(sessions.getAuthSessionState().status).toBe('completed');
  });

  it('uses an external browser for OnlyFans and persists only the captured session', async () => {
    const browser = createBrowserWindowMock();
    let resolveCompletion;
    const completion = new Promise((resolve) => { resolveCompletion = resolve; });
    const close = vi.fn(async () => {});
    const launchOnlyFansAuth = vi.fn(async () => ({ completion, close }));
    const { sessions, persistCredential } = createHarness(browser, { launchOnlyFansAuth });

    const state = await sessions.startAuthSession('onlyfans');
    expect(state.status).toBe('active');
    expect(browser.instances).toHaveLength(0);
    resolveCompletion({
      site_category: 'onlyfans',
      credential_type: 'cookies',
      cookies: { sess: 'session', auth_id: '42' },
      headers: { 'x-bc': 'signature', 'user-agent': 'Chrome' },
    });
    await settle();

    expect(persistCredential).toHaveBeenCalledWith(expect.objectContaining({
      site_id: 'onlyfans',
      cookies: { sess: 'session', auth_id: '42' },
      headers: { 'x-bc': 'signature', 'user-agent': 'Chrome' },
    }));
    expect(close).toHaveBeenCalledOnce();
    expect(sessions.getAuthSessionState().status).toBe('completed');
  });

  it('validates and saves a manually supplied OnlyFans session', async () => {
    const browser = createBrowserWindowMock();
    const { sessions, persistCredential } = createHarness(browser);

    await sessions.saveManualOnlyFansCredential({
      cookie: 'analytics=ignored; sess=session; auth_id=42; auth_uid=42',
      user_agent: 'Chrome browser',
      x_bc: 'signature',
    });

    expect(persistCredential).toHaveBeenCalledWith(expect.objectContaining({
      site_id: 'onlyfans',
      credential_type: 'cookies',
      cookies: { sess: 'session', auth_id: '42', auth_uid: '42' },
      headers: { 'x-bc': 'signature', 'user-agent': 'Chrome browser' },
    }));
    expect(sessions.getAuthSessionState().status).toBe('completed');
  });

  it('does not save a FANBOX cookie before the login redirects to FANBOX', async () => {
    const browser = createBrowserWindowMock({
      cookies: [
        { name: 'FANBOXSESSID', value: 'session', domain: '.fanbox.cc' },
      ],
    });
    const { sessions, persistCredential } = createHarness(browser);

    await sessions.startAuthSession('fanbox');
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await settle();

    expect(persistCredential).not.toHaveBeenCalled();
    expect(sessions.getAuthSessionState().status).toBe('active');
  });

  it('saves a FANBOX cookie after the login redirects to FANBOX', async () => {
    const browser = createBrowserWindowMock({
      pageResult: true,
      cookies: [
        { name: 'FANBOXSESSID', value: 'valid', domain: '.fanbox.cc' },
        { name: '__cf_bm', value: 'browser-session', domain: '.fanbox.cc' },
      ],
    });
    const { sessions, persistCredential } = createHarness(browser);

    await sessions.startAuthSession('fanbox');
    browser.instances[0].loadedUrl = 'https://www.fanbox.cc/';
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await settle();

    expect(persistCredential).toHaveBeenCalledWith(expect.objectContaining({
      site_id: 'fanbox',
      credential_type: 'cookies',
      cookies: { FANBOXSESSID: 'valid', __cf_bm: 'browser-session' },
    }));
    expect(sessions.getAuthSessionState().status).toBe('completed');
  });

  it('does not treat FANBOX guest cookies as an authenticated login', async () => {
    const browser = createBrowserWindowMock({
      pageResult: false,
      cookies: [
        { name: 'FANBOXSESSID', value: 'guest', domain: '.fanbox.cc' },
      ],
    });
    const { sessions, persistCredential } = createHarness(browser);

    await sessions.startAuthSession('fanbox');
    browser.instances[0].loadedUrl = 'https://www.fanbox.cc/';
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await settle();

    expect(persistCredential).not.toHaveBeenCalled();
    expect(sessions.getAuthSessionState().status).toBe('active');
  });

  it('reads booru API credentials from account settings after login', async () => {
    const browser = createBrowserWindowMock({
      pageResult: (url) => url.includes('s=options')
        ? {
            href: url,
            title: 'Options',
            acceptedConsent: false,
            hasChallenge: false,
            hasLoginForm: false,
            authenticated: true,
            onOptions: true,
            apiKey: 'a'.repeat(32),
            userId: '599664',
          }
        : {
            href: url,
            title: 'Account',
            acceptedConsent: false,
            hasChallenge: false,
            hasLoginForm: false,
            authenticated: true,
            onOptions: false,
            apiKey: null,
            userId: null,
          },
    });
    const { sessions, persistCredential } = createHarness(browser);

    await sessions.startAuthSession('rule34');
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await settle();
    expect(browser.instances[0].loadedUrl).toContain('s=options');
    expect(browser.instances[0].hideCalls).toBe(1);

    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await settle();
    expect(persistCredential).toHaveBeenCalledWith(expect.objectContaining({
      site_id: 'rule34',
      credential_type: 'api_key',
      username: '599664',
      password: 'a'.repeat(32),
    }));
  });

  it('keeps browser challenges active without saving a false login', async () => {
    const browser = createBrowserWindowMock({
      pageResult: {
        href: 'https://rule34.xxx/challenge',
        title: 'Just a moment',
        acceptedConsent: false,
        hasChallenge: true,
        hasLoginForm: false,
        authenticated: false,
        onOptions: false,
        apiKey: null,
        userId: null,
      },
    });
    const { sessions, persistCredential } = createHarness(browser);

    await sessions.startAuthSession('rule34');
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await settle();

    expect(persistCredential).not.toHaveBeenCalled();
    expect(sessions.getAuthSessionState()).toMatchObject({ status: 'active' });
    expect(sessions.getAuthSessionState().message).toContain('browser check');
  });

  it('persists OAuth2 results through the same host boundary', async () => {
    const fetchImpl = vi.fn()
      .mockResolvedValueOnce({ ok: true, json: async () => ({ client_id: 'client', client_secret: 'secret' }) })
      .mockResolvedValueOnce({ ok: true, json: async () => ({ access_token: 'access' }) });
    const browser = createBrowserWindowMock();
    const { sessions, persistCredential } = createHarness(browser, { fetchImpl });

    await sessions.startAuthSession('baraag');
    const authUrl = new URL(browser.instances[0].loadedUrl);
    const callback = `https://picto.app/oauth/callback?code=code&state=${authUrl.searchParams.get('state')}`;
    await browser.instances[0].webContents.listeners.get('will-redirect')({ preventDefault: vi.fn() }, callback);
    await settle();

    expect(persistCredential).toHaveBeenCalledWith(expect.objectContaining({
      site_id: 'baraag', credential_type: 'oauth_token', oauth_token: 'access',
    }));
  });

  it('uses gallery-dl DeviantArt OAuth and keeps its refresh token plus session cookies', async () => {
    const fetchImpl = vi.fn().mockResolvedValueOnce({
      ok: true,
      json: async () => ({ access_token: 'temporary-access', refresh_token: 'durable-refresh' }),
    });
    const browser = createBrowserWindowMock({
      cookies: [
        { name: 'auth', value: 'auth-cookie' },
        { name: 'auth_secure', value: 'secure-cookie' },
        { name: 'userinfo', value: 'user-cookie' },
        { name: 'unrelated', value: 'ignored' },
      ],
    });
    const { sessions, persistCredential } = createHarness(browser, { fetchImpl });

    await sessions.startAuthSession('deviantart');
    const authUrl = new URL(browser.instances[0].loadedUrl);
    expect(authUrl.origin + authUrl.pathname).toBe('https://www.deviantart.com/oauth2/authorize');
    expect(authUrl.searchParams.get('redirect_uri')).toBe('https://mikf.github.io/gallery-dl/oauth-redirect.html');
    const callback = `https://mikf.github.io/gallery-dl/oauth-redirect.html?code=code&state=${authUrl.searchParams.get('state')}`;
    await browser.instances[0].webContents.listeners.get('will-redirect')({ preventDefault: vi.fn() }, callback);
    await settle();

    expect(fetchImpl).toHaveBeenCalledOnce();
    expect(persistCredential).toHaveBeenCalledWith(expect.objectContaining({
      site_id: 'deviantart',
      credential_type: 'oauth_token',
      oauth_token: 'durable-refresh',
      cookies: {
        auth: 'auth-cookie',
        auth_secure: 'secure-cookie',
        userinfo: 'user-cookie',
      },
    }));
  });

  it('persists both OAuth1 token values through the same host boundary', async () => {
    const fetchImpl = vi.fn()
      .mockResolvedValueOnce({ ok: true, text: async () => 'oauth_token=request&oauth_token_secret=request-secret' })
      .mockResolvedValueOnce({ ok: true, text: async () => 'oauth_token=access&oauth_token_secret=access-secret' });
    const browser = createBrowserWindowMock();
    const { sessions, persistCredential } = createHarness(browser, { fetchImpl });

    await sessions.startAuthSession('tumblr');
    await browser.instances[0].webContents.listeners.get('will-navigate')(
      { preventDefault: vi.fn() },
      'https://picto.app/oauth/callback?oauth_token=request&oauth_verifier=verified',
    );
    await settle();

    expect(persistCredential).toHaveBeenCalledWith(expect.objectContaining({
      site_id: 'tumblr', credential_type: 'oauth_token', oauth_token: 'access', password: 'access-secret',
    }));
  });

  it('uses one host-owned Pixiv OAuth flow for Pixiv and Pixiv-user queries', async () => {
    const browser = createBrowserWindowMock({
      cookies: [{ name: 'PHPSESSID', value: 'pixiv-session' }],
    });
    const { sessions, beginPixivOAuth, completePixivOAuth, persistCredential } = createHarness(browser);

    const state = await sessions.startAuthSession('pixivuser');
    expect(state).toMatchObject({ site_category: 'pixiv', status: 'active' });
    expect(beginPixivOAuth).toHaveBeenCalledOnce();

    await browser.instances[0].webContents.listeners.get('will-redirect')(
      { preventDefault: vi.fn() },
      'pixiv://account/login?code=oauth-code',
    );
    await settle();

    expect(completePixivOAuth).toHaveBeenCalledWith({
      code: 'oauth-code', code_verifier: 'verifier', phpsessid: 'pixiv-session',
    });
    expect(persistCredential).not.toHaveBeenCalled();
    expect(sessions.getAuthSessionState().status).toBe('completed');
  });

  it('retains active state for renderer remounts and cancels cleanly', async () => {
    const browser = createBrowserWindowMock();
    const { sessions } = createHarness(browser);

    await sessions.startAuthSession('deviantart');
    expect(sessions.getAuthSessionState()).toMatchObject({ site_category: 'deviantart', status: 'active' });

    await sessions.cancelAuthSession();
    expect(sessions.getAuthSessionState()).toMatchObject({ site_category: null, status: 'idle' });
  });

  it('reports main-frame load failures without completing authentication', async () => {
    const browser = createBrowserWindowMock();
    const { sessions, persistCredential } = createHarness(browser);
    await sessions.startAuthSession('patreon');

    browser.instances[0].webContents.listeners.get('did-fail-load')(
      null, -105, 'NAME_NOT_RESOLVED', 'https://www.patreon.com/login', true,
    );

    expect(sessions.getAuthSessionState()).toMatchObject({ status: 'error' });
    expect(sessions.getAuthSessionState().message).toContain('NAME_NOT_RESOLVED');
    expect(persistCredential).not.toHaveBeenCalled();
  });
});
