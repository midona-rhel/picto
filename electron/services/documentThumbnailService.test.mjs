import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createDocumentThumbnailService } from './documentThumbnailService.mjs';

const temporaryDirectories = [];

afterEach(async () => {
  await Promise.all(temporaryDirectories.splice(0).map((directory) => fs.rm(directory, { recursive: true, force: true })));
});

describe('document thumbnail service', () => {
  it('captures the rendered document page without its viewport or footer', async () => {
    const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-document-thumbnail-'));
    temporaryDirectories.push(directory);
    const outputPath = path.join(directory, 'thumbnail.png');
    const windows = [];
    class FakeBrowserWindow {
      constructor(options) {
        this.options = options;
        this.destroyed = false;
        this.webContents = {
          setWindowOpenHandler: (handler) => { this.windowOpenHandler = handler; },
          on: (event, handler) => { this.webContentsHandlers = { ...this.webContentsHandlers, [event]: handler }; },
          executeJavaScript: async () => ({ ready: true, x: 256, y: 24, width: 816, height: 1056 }),
          capturePage: async (rect) => {
            this.captureRect = rect;
            return { toPNG: () => Buffer.from('document-page') };
          },
        };
        windows.push(this);
      }
      async loadURL(url) { this.url = url; }
      isDestroyed() { return this.destroyed; }
      destroy() { this.destroyed = true; }
    }
    const service = createDocumentThumbnailService({
      BrowserWindow: FakeBrowserWindow,
      app: { getAppPath: () => '/unused' },
      path,
      isDev: true,
      devUrl: 'http://127.0.0.1:8080',
    });
    await service.render({ hash: 'a'.repeat(64), mimeType: 'application/rtf', outputPath });
    expect(windows[0].options.show).toBe(false);
    expect(windows[0].options.width).toBe(1328);
    expect(windows[0].options.height).toBe(1200);
    expect(windows[0].windowOpenHandler()).toEqual({ action: 'deny' });
    const navigationEvent = { preventDefault: vi.fn() };
    windows[0].webContentsHandlers['will-navigate'](navigationEvent);
    expect(navigationEvent.preventDefault).toHaveBeenCalledOnce();
    expect(windows[0].url).toContain('document-thumbnail.html?hash=');
    expect(windows[0].url).toContain('mime=application%2Frtf');
    expect(windows[0].captureRect).toEqual({ x: 256, y: 24, width: 816, height: 1056 });
    expect(windows[0].destroyed).toBe(true);
    expect(await fs.readFile(outputPath, 'utf8')).toBe('document-page');
  });
});
