import { EventEmitter } from 'node:events';
import { describe, expect, it, vi } from 'vitest';
import { runReverseImageSearch } from './registerHandlers.mjs';

class FakeWebContents extends EventEmitter {
  constructor() {
    super();
    this.url = '';
    this.executedScripts = [];
    this.debugger = {
      attach: vi.fn(),
      detach: vi.fn(),
      sendCommand: vi.fn(async (command) => {
        if (command === 'DOM.getDocument') return { root: { nodeId: 1 } };
        if (command === 'DOM.querySelector') return { nodeId: 2 };
        return {};
      }),
    };
  }

  setWindowOpenHandler(handler) { this.windowOpenHandler = handler; }
  async loadURL(url) { this.url = url; }
  async executeJavaScript(script) {
    this.executedScripts.push(script);
    if (script.includes('Google Lens did not receive the image')) {
      this.url = 'https://lens.google.com/search?p=fixture';
    }
    if (script.includes('Bing did not receive the image')) {
      this.url = 'https://www.bing.com/images/searchbyimage/upload';
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
  it('uploads to Google Lens invisibly and opens the result externally', async () => {
    const openExternal = vi.fn(async () => {});

    await expect(runReverseImageSearch({
      BrowserWindow: FakeBrowserWindow,
      filePath: '/tmp/image.jpg',
      engine: 'google',
      openExternal,
    })).resolves.toBe('https://lens.google.com/search?p=fixture');

    expect(FakeBrowserWindow.instance.options.show).toBe(false);
    expect(FakeBrowserWindow.instance.webContents.executedScripts[0])
      .toContain('[jsname="R5mgy"][role="button"]');
    expect(openExternal).toHaveBeenCalledWith('https://lens.google.com/search?p=fixture');
    expect(FakeBrowserWindow.instance.destroyed).toBe(true);
  });

  it('keeps the upload surface hidden and opens Bing results externally', async () => {
    const openExternal = vi.fn(async () => {});

    await expect(runReverseImageSearch({
      BrowserWindow: FakeBrowserWindow,
      filePath: '/tmp/image.jpg',
      engine: 'bing',
      openExternal,
    })).resolves.toBe('https://www.bing.com/images/searchbyimage/upload');

    expect(FakeBrowserWindow.instance.options.show).toBe(false);
    expect(openExternal).toHaveBeenCalledWith('https://www.bing.com/images/searchbyimage/upload');
    expect(FakeBrowserWindow.instance.destroyed).toBe(true);
  });
});
