import { describe, expect, it } from 'vitest';
import { createAuthSessions } from './authSessions.mjs';

function createBrowserWindowMock({ pageResult = null, cookies = [] } = {}) {
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
        setWindowOpenHandler: () => {},
        getURL: () => this.loadedUrl ?? '',
        getTitle: () => '',
        isDestroyed: () => false,
        executeJavaScript: async (script) => {
          // Electron parses this generated page script before executing it.
          // Keep the unit test honest about escaping errors in the template.
          new Function(script);
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
            get: async () => cookies,
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

  it('does not create an auth session for anonymous Danbooru', async () => {
    const browser = createBrowserWindowMock();
    const sessions = createAuthSessions({ BrowserWindow: browser.BrowserWindow, getMainWindow: () => null });

    await expect(sessions.startAuthSession('danbooru')).rejects.toThrow(
      'Unsupported auth site: danbooru',
    );
    expect(browser.instances).toHaveLength(0);
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
