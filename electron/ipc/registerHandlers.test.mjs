import { EventEmitter } from 'node:events';
import { fileURLToPath } from 'node:url';
import { describe, expect, it, vi } from 'vitest';
import { closeDesktopWindow, registerIpcHandlers, runReverseImageSearch } from './registerHandlers.mjs';

describe('native grid cancellation transport', () => {
  function handler(invokeSerialized) {
    const handlers = new Map();
    registerIpcHandlers({
      ipcMain: { handle: (name, callback) => handlers.set(name, callback), on: vi.fn() },
      windowManager: { ownsWebContents: () => true },
      invokeSerialized,
    });
    return (command) => handlers.get('picto:invoke')({ sender: {} }, { command, args: {} });
  }

  it('resolves expected supersession instead of making Electron log a handler failure', async () => {
    const invoke = handler(vi.fn().mockRejectedValue(new Error('query superseded')));
    await expect(invoke('items.window')).resolves.toEqual({ __pictoQuerySuperseded: true });
    await expect(invoke('items.delete')).rejects.toThrow('query superseded');
  });

  it('does not suppress a genuine window read failure', async () => {
    await expect(handler(vi.fn().mockRejectedValue(new Error('database unavailable')))('items.window'))
      .rejects.toThrow('database unavailable');
  });
});

describe('closeDesktopWindow', () => {
  it.each(['win32', 'linux'])('quits when closing the main window on %s', (platform) => {
    const main = { close: vi.fn() };
    const app = { quit: vi.fn() };
    closeDesktopWindow({ app, windowManager: { getMainWindow: () => main }, win: main, platform });
    expect(app.quit).toHaveBeenCalledOnce();
    expect(main.close).not.toHaveBeenCalled();
  });

  it('keeps the application alive when closing the main window on macOS', () => {
    const main = { close: vi.fn() };
    const app = { quit: vi.fn() };
    closeDesktopWindow({ app, windowManager: { getMainWindow: () => main }, win: main, platform: 'darwin' });
    expect(main.close).toHaveBeenCalledOnce();
    expect(app.quit).not.toHaveBeenCalled();
  });

  it('closes secondary windows without quitting', () => {
    const main = {};
    const secondary = { close: vi.fn() };
    const app = { quit: vi.fn() };
    closeDesktopWindow({ app, windowManager: { getMainWindow: () => main }, win: secondary, platform: 'win32' });
    expect(secondary.close).toHaveBeenCalledOnce();
    expect(app.quit).not.toHaveBeenCalled();
  });
});

class FakeWebContents extends EventEmitter {
  constructor() {
    super();
    this.url = '';
    this.loadedUrls = [];
    this.executedScripts = [];
    this.debugger = {
      attach: vi.fn(),
      detach: vi.fn(),
      sendCommand: vi.fn(async (command) => {
        if (command === 'DOM.getDocument') return { root: { nodeId: 1 } };
        if (command === 'DOM.querySelector') return { nodeId: 2 };
        if (command === 'DOM.setFileInputFiles' && this.url.includes('bing.com/images')) {
          this.url = 'https://www.bing.com/search?q=fixture&bcid=fixture&FORM=SBIIRP';
        }
        return {};
      }),
    };
  }

  setWindowOpenHandler(handler) { this.windowOpenHandler = handler; }
  async loadURL(url) {
    this.loadedUrls.push(url);
    this.url = url;
  }
  async executeJavaScript(script) {
    this.executedScripts.push(script);
    if (script.includes("document.querySelector('#searchForm')?.submit()")) {
      this.url = 'https://saucenao.com/search.php?db=999';
    }
    if (script.includes("const thumbnail = await __waitFor('#yourimage a img')")) {
      return 'https://saucenao.com/search.php?db=999&url=https%3A%2F%2Fsaucenao.com%2Ffixture.png';
    }
  }
  getURL() { return this.url; }
}

class FakeBrowserWindow extends EventEmitter {
  constructor(options) {
    super();
    this.options = options;
    this.webContents = new FakeWebContents();
    this.destroyed = false;
    FakeBrowserWindow.instance = this;
  }

  isDestroyed() { return this.destroyed; }
  destroy() { this.destroyed = true; }
}

describe('runReverseImageSearch', () => {
  it('submits SauceNAO through its native form and opens the result externally', async () => {
    const openExternal = vi.fn(async () => {});

    await expect(runReverseImageSearch({
      BrowserWindow: FakeBrowserWindow,
      filePath: fileURLToPath(import.meta.url),
      engine: 'saucenao',
      openExternal,
    })).resolves.toBe('https://saucenao.com/search.php?db=999&url=https%3A%2F%2Fsaucenao.com%2Ffixture.png');

    expect(FakeBrowserWindow.instance.webContents.executedScripts.some(
      (script) => script.includes("document.querySelector('#searchForm')?.submit()"),
    )).toBe(true);
    expect(openExternal).toHaveBeenCalledWith(
      'https://saucenao.com/search.php?db=999&url=https%3A%2F%2Fsaucenao.com%2Ffixture.png',
    );
  });

  it('keeps the upload surface hidden and opens Bing results externally', async () => {
    const openExternal = vi.fn(async () => {});

    await expect(runReverseImageSearch({
      BrowserWindow: FakeBrowserWindow,
      filePath: '/tmp/image.jpg',
      engine: 'bing',
      openExternal,
    })).resolves.toBe('https://www.bing.com/search?q=fixture&bcid=fixture&FORM=SBIIRP');

    expect(FakeBrowserWindow.instance.options.show).toBe(false);
    expect(openExternal).toHaveBeenCalledWith('https://www.bing.com/search?q=fixture&bcid=fixture&FORM=SBIIRP');
    expect(FakeBrowserWindow.instance.destroyed).toBe(true);
  });
});
