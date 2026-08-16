import { describe, expect, it, vi } from 'vitest';
import { JSDOM } from 'jsdom';
import { createAuthSessions } from './authSessions.mjs';

function createBrowserWindowMock({ pageResult = null, pageHtml = null, cookies = [] } = {}) {
  const instances = [];

  class FakeBrowserWindow {
    constructor(options) {
      this.options = options;
      this.listeners = new Map();
      this.messages = [];
      this.hideCalls = 0;
      this.showCalls = 0;
      this.webContents = {
        loadURL: async (url) => { this.loadedUrl = url; },
        on: (event, handler) => { this.webContents.listeners.set(event, handler); },
        listeners: new Map(),
        send: (_channel, payload) => { this.messages.push(payload); },
        getUserAgent: () => 'Mozilla/5.0 Chrome/138.0.0.0 Safari/537.36 Electron/37.0.0 Picto/0.5.0',
        setUserAgent: (userAgent) => { this.userAgent = userAgent; },
        setWindowOpenHandler: (handler) => { this.windowOpenHandler = handler; },
        getURL: () => this.loadedUrl ?? '',
        getTitle: () => '',
        isDestroyed: () => false,
        executeJavaScript: async (script) => {
          // Electron parses this generated page script before executing it.
          // Keep the unit test honest about escaping errors in the template.
          new Function(script);
          if (pageHtml != null) {
            const dom = new JSDOM(pageHtml, { url: this.loadedUrl || 'https://example.test/' });
            return new Function('document', 'location', `return (${script.trim()})`)(dom.window.document, dom.window.location);
          }
          return (typeof pageResult === 'function' ? pageResult(this.loadedUrl) : pageResult) ?? ({
            href: this.loadedUrl ?? '',
            title: 'Rule34 account options',
            hasLoginForm: false,
            hasLogoutLink: true,
            hasAccountHome: true,
            hasAccountOptions: true,
            apiKey: 'a'.repeat(32),
            userId: '34',
          });
        },
        session: {
          clearStorageData: async () => {},
          cookies: {
            get: async () => (typeof cookies === 'function' ? cookies() : cookies),
          },
        },
      };
      instances.push(this);
    }

    once(event, handler) {
      this.listeners.set(event, handler);
    }

    on(event, handler) {
      this.listeners.set(event, handler);
    }

    isDestroyed() {
      return false;
    }

    close() { this.listeners.get('closed')?.(); }

    focus() {}

    hide() { this.hideCalls += 1; }

    show() { this.showCalls += 1; }
  }

  FakeBrowserWindow.getAllWindows = () => instances;

  return { BrowserWindow: FakeBrowserWindow, instances };
}

describe('auth session routing', () => {
  it('rejects unsupported sites before creating a window', async () => {
    const browser = createBrowserWindowMock();
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    await expect(sessions.startAuthSession('unsupported-site')).rejects.toThrow(
      'Unsupported auth site: unsupported-site',
    );
    expect(browser.instances).toHaveLength(0);
  });

  it('opens a direct-site session for optional Danbooru login', async () => {
    const browser = createBrowserWindowMock();
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    const state = await sessions.startAuthSession('danbooru');

    expect(browser.instances).toHaveLength(1);
    expect(browser.instances[0].loadedUrl).toBe('https://danbooru.donmai.us/session/new');
    expect(state).toMatchObject({ site_category: 'danbooru', status: 'active' });
    await sessions.cancelAuthSession();
  });

  it('exposes the current session so a remounted renderer can resume the handoff', async () => {
    const browser = createBrowserWindowMock();
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    await sessions.startAuthSession('deviantart');
    expect(sessions.getAuthSessionState()).toMatchObject({
      site_category: 'deviantart',
      status: 'active',
    });

    await sessions.cancelAuthSession();
    expect(sessions.getAuthSessionState()).toMatchObject({
      site_category: null,
      status: 'idle',
    });
  });

  it('keeps HTTPS signup links inside the isolated auth window', async () => {
    const browser = createBrowserWindowMock();
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });
    await sessions.startAuthSession('webtoons');

    expect(browser.instances[0].windowOpenHandler({ url: 'https://www.webtoons.com/member/join' })).toEqual({ action: 'deny' });
    await new Promise((resolve) => queueMicrotask(resolve));
    expect(browser.instances[0].loadedUrl).toBe('https://www.webtoons.com/member/join');

    expect(browser.instances[0].windowOpenHandler({ url: 'file:///tmp/not-allowed' })).toEqual({ action: 'deny' });
    await sessions.cancelAuthSession();
  });

  it.each([
    ['webtoons', 'https://www.webtoons.com/member/login'],
    ['deviantart', 'https://www.deviantart.com/users/login'],
    ['idolcomplex', 'https://login.idol.sankakucomplex.com/oidc/auth?response_type=code&scope=openid&client_id=idol-web-app&redirect_uri=https%3A%2F%2Fwww.idolcomplex.com%2Fsso%2Fcallback&state=return_uri%3Dhttps%3A%2F%2Fwww.idolcomplex.com%2Fen%2Flogin&theme=black&route=login&lang=en'],
    ['sankaku', 'https://login.sankakucomplex.com/oidc/auth?response_type=code&scope=openid&client_id=sankaku-web-app&redirect_uri=https%3A%2F%2Fsankaku.app%2Fsso%2Fcallback&state=return_uri%3Dhttps%3A%2F%2Fsankaku.app%2F&theme=black&route=login&lang=en'],
    ['yandere', 'https://yande.re/user/login'],
    ['konachan', 'https://konachan.com/user/login'],
    ['safebooru', 'https://safebooru.org/index.php?page=account&s=login&code=00'],
    ['e621', 'https://e621.net/session/new'],
  ])('opens the direct-site session for %s', async (site, loginUrl) => {
    const browser = createBrowserWindowMock();
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    const state = await sessions.startAuthSession(site);

    expect(browser.instances).toHaveLength(1);
    expect(browser.instances[0].loadedUrl).toBe(loginUrl);
    expect(state).toMatchObject({ site_category: site, status: 'active' });
    await sessions.cancelAuthSession();
  });

  it('registers Baraag, opens its real authorization page, and captures the access token', async () => {
    const fetchImpl = vi.fn()
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: async () => ({ client_id: 'baraag-client', client_secret: 'baraag-secret' }),
      })
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        json: async () => ({ access_token: 'baraag-access-token' }),
      });
    const browser = createBrowserWindowMock();
    const sessions = createAuthSessions({
      BrowserWindow: browser.BrowserWindow,
      getMainWindow: () => null,
      fetchImpl,
    });

    const state = await sessions.startAuthSession('baraag');
    const authorizationUrl = new URL(browser.instances[0].loadedUrl);
    expect(state).toMatchObject({ site_category: 'baraag', status: 'active' });
    expect(authorizationUrl.origin).toBe('https://baraag.net');
    expect(authorizationUrl.pathname).toBe('/oauth/authorize');
    expect(authorizationUrl.searchParams.get('redirect_uri')).toBe('https://picto.app/oauth/callback');
    expect(authorizationUrl.searchParams.get('scope')).toBe('read');

    let prevented = false;
    await browser.instances[0].webContents.listeners.get('will-redirect')(
      { preventDefault: () => { prevented = true; } },
      `https://picto.app/oauth/callback?code=baraag-code&state=${authorizationUrl.searchParams.get('state')}`,
    );
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(prevented).toBe(true);
    expect(fetchImpl).toHaveBeenCalledTimes(2);
    expect(fetchImpl.mock.calls[1][0]).toBe('https://baraag.net/oauth/token');
    expect(fetchImpl.mock.calls[1][1].body).toContain('code=baraag-code');
    expect(browser.instances[0].messages.at(-1)?.credential).toEqual({
      site_category: 'baraag',
      credential_type: 'oauth_token',
      oauth_token: 'baraag-access-token',
    });
  });

  it('uses gallery-dl Tumblr OAuth1 credentials and captures both access token values', async () => {
    const fetchImpl = vi.fn()
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        text: async () => 'oauth_token=request-token&oauth_token_secret=request-secret&oauth_callback_confirmed=true',
      })
      .mockResolvedValueOnce({
        ok: true,
        status: 200,
        text: async () => 'oauth_token=access-token&oauth_token_secret=access-secret',
      });
    const browser = createBrowserWindowMock();
    const sessions = createAuthSessions({
      BrowserWindow: browser.BrowserWindow,
      getMainWindow: () => null,
      fetchImpl,
    });

    await sessions.startAuthSession('tumblr');
    const authorizationUrl = new URL(browser.instances[0].loadedUrl);
    expect(authorizationUrl.toString()).toBe('https://www.tumblr.com/oauth/authorize?oauth_token=request-token&perms=read');
    expect(fetchImpl.mock.calls[0][1].headers.Authorization).toContain('oauth_consumer_key="O3hU2tMi5e4Qs5t3vezEi6L0qRORJ5y9oUpSGsrWu8iA3UCc3B"');

    let prevented = false;
    await browser.instances[0].webContents.listeners.get('will-navigate')(
      { preventDefault: () => { prevented = true; } },
      'https://picto.app/oauth/callback?oauth_token=request-token&oauth_verifier=verifier',
    );
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(prevented).toBe(true);
    expect(fetchImpl).toHaveBeenCalledTimes(2);
    expect(fetchImpl.mock.calls[1][1].body).toBe('oauth_token=request-token&oauth_verifier=verifier');
    expect(browser.instances[0].messages.at(-1)?.credential).toEqual({
      site_category: 'tumblr',
      credential_type: 'oauth_token',
      oauth_token: 'access-token',
      password: 'access-secret',
    });
  });

  it('captures the resulting cookies from a direct-site login', async () => {
    const browser = createBrowserWindowMock({
      pageResult: true,
      cookies: [
        { name: '_session', value: 'authenticated' },
        { name: 'user', value: '123' },
      ],
    });
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    await sessions.startAuthSession('danbooru');
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await new Promise((resolve) => setTimeout(resolve, 0));

    const completed = browser.instances[0].messages.find((message) => message.status === 'completed');
    expect(completed?.credential).toEqual({
      site_category: 'danbooru',
      credential_type: 'cookies',
      cookies: { _session: 'authenticated', user: '123' },
    });
  });

  it.each(['idolcomplex', 'sankaku'])(
    'captures %s from its OIDC cookies without relying on rendered account UI',
    async (site) => {
    const browser = createBrowserWindowMock({
      cookies: [
        { name: 'accessToken', value: 'access-token' },
        { name: 'refreshToken', value: 'refresh-token' },
        { name: 'ssoLoginValid', value: 'true' },
        { name: '_pk_id', value: 'analytics' },
      ],
    });
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    await sessions.startAuthSession(site);
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await new Promise((resolve) => setTimeout(resolve, 0));

    const completed = browser.instances[0].messages.find((message) => message.status === 'completed');
    expect(completed?.credential).toEqual({
      site_category: site,
      credential_type: 'cookies',
      cookies: {
        accessToken: 'access-token',
        refreshToken: 'refresh-token',
        ssoLoginValid: 'true',
      },
    });
    },
  );

  it('periodically inspects known authentication cookies after login completes', async () => {
    vi.useFakeTimers();
    try {
      let authenticated = false;
      const browser = createBrowserWindowMock({
        pageResult: false,
        cookies: () => authenticated
          ? [{ name: 'a', value: 'session-a' }, { name: 'b', value: 'session-b' }]
          : [],
      });
      const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

      await sessions.startAuthSession('furaffinity');
      await browser.instances[0].webContents.listeners.get('did-finish-load')();
      await vi.advanceTimersByTimeAsync(0);
      expect(browser.instances[0].messages.some((message) => message.status === 'completed')).toBe(false);

      authenticated = true;
      await vi.advanceTimersByTimeAsync(750);

      const completed = browser.instances[0].messages.find((message) => message.status === 'completed');
      expect(completed?.credential).toEqual({
        site_category: 'furaffinity',
        credential_type: 'cookies',
        cookies: { a: 'session-a', b: 'session-b' },
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it('uses the canonical Pixiv session for Pixiv-user queries', async () => {
    const browser = createBrowserWindowMock();
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    const state = await sessions.startAuthSession('pixivuser', 'https://pixiv.example/login');

    expect(browser.instances).toHaveLength(1);
    expect(browser.instances[0].loadedUrl).toBe('https://pixiv.example/login');
    expect(state.site_category).toBe('pixiv');
    expect(state.status).toBe('active');
    await sessions.cancelAuthSession();
  });

  it('opens the direct Pixiv login session for Pixiv searches', async () => {
    const browser = createBrowserWindowMock();
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    const state = await sessions.startAuthSession('pixiv', 'https://app-api.pixiv.net/web/v1/login');

    expect(browser.instances).toHaveLength(1);
    expect(browser.instances[0].loadedUrl).toBe('https://app-api.pixiv.net/web/v1/login');
    expect(state).toMatchObject({ site_category: 'pixiv', status: 'active' });
    await sessions.cancelAuthSession();
  });

  it('opens the Gelbooru API-key session', async () => {
    const browser = createBrowserWindowMock();
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    const state = await sessions.startAuthSession('gelbooru');

    expect(browser.instances).toHaveLength(1);
    expect(browser.instances[0].loadedUrl).toContain('gelbooru.com');
    expect(state.site_category).toBe('gelbooru');
    expect(state.status).toBe('active');
    await sessions.cancelAuthSession();
  });

  it('opens the separate Rule34 API-key session', async () => {
    const browser = createBrowserWindowMock();
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    const state = await sessions.startAuthSession('rule34');

    expect(browser.instances).toHaveLength(1);
    expect(browser.instances[0].loadedUrl).toBe('https://rule34.xxx/index.php?code=00&page=account&s=login');
    expect(browser.instances[0].options.webPreferences.sandbox).toBe(true);
    expect(browser.instances[0].options.webPreferences.partition).toBe('persist:picto-auth-v1-rule34');
    expect(browser.instances[0].userAgent).toBeUndefined();
    expect(state.site_category).toBe('rule34');
    expect(state.status).toBe('active');
    await sessions.cancelAuthSession();
  });

  it('captures Rule34 credentials with the Rule34 site category', async () => {
    const browser = createBrowserWindowMock();
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    await sessions.startAuthSession('rule34');
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await new Promise((resolve) => setTimeout(resolve, 0));

    const completed = browser.instances[0].messages.find((message) => message.status === 'completed');
    expect(completed).toMatchObject({
      site_category: 'rule34',
      credential: {
        site_category: 'rule34',
        credential_type: 'api_key',
        username: '34',
        password: 'a'.repeat(32),
      },
    });
  });

  it('uses the authenticated Rule34 browser session to read account options', async () => {
    const browser = createBrowserWindowMock({
      pageResult: (url) => url.includes('s=options')
        ? {
            href: url,
            title: 'Account Options',
            hasLoginForm: false,
            hasLogoutLink: true,
            hasAccountHome: false,
            hasAccountOptions: true,
            hasChallenge: false,
            apiKey: 'b'.repeat(32),
            userId: '9876',
          }
        : {
            href: url,
            title: 'My Account',
            hasLoginForm: false,
            hasLogoutLink: true,
            hasAccountHome: true,
            hasAccountOptions: false,
            hasChallenge: false,
            apiKey: null,
            userId: null,
          },
    });
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    await sessions.startAuthSession('rule34');
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(browser.instances[0].loadedUrl).toBe('https://rule34.xxx/index.php?page=account&s=options');
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await new Promise((resolve) => setTimeout(resolve, 0));

    const completed = browser.instances[0].messages.find((message) => message.status === 'completed');
    expect(completed?.credential).toMatchObject({
      site_category: 'rule34',
      credential_type: 'api_key',
      username: '9876',
      password: 'b'.repeat(32),
    });
  });

  it('hides the Rule34 window while reading account settings', async () => {
    const browser = createBrowserWindowMock({
      pageResult: {
        href: 'https://rule34.xxx/index.php?page=account&s=login',
        title: 'My Account',
        hasLoginForm: false,
        hasLogoutLink: true,
        hasAccountHome: true,
        hasAccountOptions: false,
        hasChallenge: false,
        apiKey: null,
        userId: null,
      },
    });
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    await sessions.startAuthSession('rule34');
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(browser.instances[0].loadedUrl).toBe('https://rule34.xxx/index.php?page=account&s=options');
    expect(browser.instances[0].hideCalls).toBe(1);
  });

  it('uses the authenticated Rule34 account URL when the page markup has no login signal', async () => {
    const browser = createBrowserWindowMock({
      pageResult: {
        href: 'https://rule34.xxx/index.php?page=account&s=home',
        title: 'My Account',
        hasLoginForm: false,
        hasLogoutLink: false,
        hasAccountHome: false,
        hasAccountOptions: false,
        hasChallenge: false,
        apiKey: null,
        userId: null,
      },
    });
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    await sessions.startAuthSession('rule34');
    browser.instances[0].loadedUrl = 'https://rule34.xxx/index.php?page=account&s=home';
    browser.instances[0].webContents.listeners.get('did-navigate')(null, browser.instances[0].loadedUrl);
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(browser.instances[0].loadedUrl).toBe('https://rule34.xxx/index.php?page=account&s=options');
    expect(browser.instances[0].hideCalls).toBe(1);
  });

  it('reports a Rule34 browser challenge without closing the login session', async () => {
    const browser = createBrowserWindowMock({
      pageResult: {
        href: 'https://rule34.xxx/index.php?page=account&s=login',
        title: 'Just a moment...',
        hasLoginForm: false,
        hasLogoutLink: false,
        hasAccountHome: false,
        hasAccountOptions: false,
        hasChallenge: true,
        apiKey: null,
        userId: null,
      },
    });
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    await sessions.startAuthSession('rule34');
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await new Promise((resolve) => setTimeout(resolve, 0));

    const challenge = browser.instances[0].messages.find((message) =>
      message.message?.includes('Complete it in the login window')
    );
    expect(challenge).toMatchObject({ site_category: 'rule34', status: 'active' });
    expect(browser.instances[0].loadedUrl).toBe('https://rule34.xxx/index.php?code=00&page=account&s=login');
  });

  it('captures cookie sessions from authenticated button and menu signals', async () => {
    const browser = createBrowserWindowMock({
      pageHtml: `
        <button aria-label="Account menu">Account</button>
        <div role="menu"><div role="menuitem">Sign out</div></div>
      `,
      cookies: [{ name: 'session', value: 'authenticated' }],
    });
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    await sessions.startAuthSession('danbooru');
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await new Promise((resolve) => setTimeout(resolve, 0));

    const completed = browser.instances[0].messages.find((message) => message.status === 'completed');
    expect(completed?.credential).toEqual({
      site_category: 'danbooru',
      credential_type: 'cookies',
      cookies: { session: 'authenticated' },
    });
  });

  it('does not treat pre-login consent cookies as an authenticated session', async () => {
    const browser = createBrowserWindowMock({
      pageHtml: '<button>Accept cookies</button><p>We use tracking cookies.</p>',
      cookies: [{ name: 'consent', value: 'accepted' }, { name: '_ga', value: 'tracking' }],
    });
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    await sessions.startAuthSession('danbooru');
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(browser.instances[0].messages.some((message) => message.status === 'completed')).toBe(false);
    await sessions.cancelAuthSession();
  });

  it('never accepts Webtoons signup pages as authenticated', async () => {
    const browser = createBrowserWindowMock({
      pageHtml: '<p>Create your profile and account</p>',
      cookies: [{ name: 'anonymous-session', value: 'not-authenticated' }],
    });
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    await sessions.startAuthSession('webtoons');
    browser.instances[0].loadedUrl = 'https://www.webtoons.com/member/join/profile';
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(browser.instances[0].messages.some((message) => message.status === 'completed')).toBe(false);
    await sessions.cancelAuthSession();
  });

  it('never accepts the Webtoons signup age gate as authenticated', async () => {
    const browser = createBrowserWindowMock({
      pageHtml: '<p>Create your profile and account</p>',
      cookies: [{ name: 'wtu', value: 'site-issued-bootstrap-cookie' }],
    });
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    await sessions.startAuthSession('webtoons');
    browser.instances[0].loadedUrl = 'https://www.webtoons.com/en/age-gate?isSignUp=true';
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(browser.instances[0].messages.some((message) => message.status === 'completed')).toBe(false);
    expect(sessions.getAuthSessionState()).toMatchObject({
      status: 'active',
      current_url: 'https://www.webtoons.com/en/age-gate?isSignUp=true',
    });
    await sessions.cancelAuthSession();
  });

  it('reports main-frame cookie-session load failures and ignores subframes', async () => {
    const browser = createBrowserWindowMock();
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    await sessions.startAuthSession('danbooru');
    const didFailLoad = browser.instances[0].webContents.listeners.get('did-fail-load');

    didFailLoad(null, -105, 'ERR_NAME_NOT_RESOLVED', 'https://danbooru.donmai.us/session/new', false);
    expect(browser.instances[0].messages.at(-1)).not.toMatchObject({ status: 'error' });

    didFailLoad(null, -105, 'ERR_NAME_NOT_RESOLVED', 'https://danbooru.donmai.us/session/new', true);
    expect(browser.instances[0].messages.at(-1)).toMatchObject({
      site_category: 'danbooru',
      status: 'error',
      current_url: 'https://danbooru.donmai.us/session/new',
    });
    expect(browser.instances[0].messages.at(-1).message).toContain('could not load');
    expect(browser.instances[0].showCalls).toBe(1);
    await sessions.cancelAuthSession();
  });

  it('captures only the Fur Affinity cookies required by gallery-dl', async () => {
    const browser = createBrowserWindowMock({
      cookies: [
        { name: 'a', value: 'session-a' },
        { name: 'b', value: 'session-b' },
        { name: 'unrelated', value: 'do-not-store' },
      ],
    });
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    await sessions.startAuthSession('furaffinity');
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await new Promise((resolve) => setTimeout(resolve, 0));

    const completed = browser.instances[0].messages.find((message) => message.status === 'completed');
    expect(completed?.credential).toEqual({
      site_category: 'furaffinity',
      credential_type: 'cookies',
      cookies: { a: 'session-a', b: 'session-b' },
    });
  });

  it('captures Hentai Foundry PHPSESSID only after the page is authenticated', async () => {
    let authenticated = false;
    const browser = createBrowserWindowMock({
      cookies: [
        { name: 'PHPSESSID', value: 'session-id' },
        { name: 'YII_CSRF_TOKEN', value: 'do-not-store' },
      ],
      pageResult: () => authenticated,
    });
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    await sessions.startAuthSession('hentaifoundry');
    expect(browser.instances[0].loadedUrl).toBe('https://www.hentai-foundry.com/site/index');
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(browser.instances[0].messages.some((message) => message.status === 'completed')).toBe(false);

    authenticated = true;
    await browser.instances[0].webContents.listeners.get('did-finish-load')();
    await new Promise((resolve) => setTimeout(resolve, 0));

    const completed = browser.instances[0].messages.find((message) => message.status === 'completed');
    expect(completed?.credential).toEqual({
      site_category: 'hentaifoundry',
      credential_type: 'cookies',
      cookies: { PHPSESSID: 'session-id' },
    });
  });
});
