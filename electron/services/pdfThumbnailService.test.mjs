import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { createPdfThumbnailService } from './pdfThumbnailService.mjs';

const temporaryDirectories = [];

afterEach(async () => {
  await Promise.all(temporaryDirectories.splice(0).map((directory) => fs.rm(directory, { recursive: true, force: true })));
});

describe('pdf thumbnail service', () => {
  it('captures the isolated first-page renderer without viewer chrome', async () => {
    const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-pdf-thumbnail-'));
    temporaryDirectories.push(directory);
    const outputPath = path.join(directory, 'thumbnail.png');
    const windows = [];

    class FakeBrowserWindow {
      constructor(options) {
        this.options = options;
        this.destroyed = false;
        this.webContents = {
          executeJavaScript: async () => ({ ready: true, width: 400, height: 600 }),
          capturePage: async (rect) => {
            this.captureRect = rect;
            return { toPNG: () => Buffer.from('pdf-page') };
          },
        };
        windows.push(this);
      }
      async loadURL(url) { this.url = url; }
      setContentSize(width, height) { this.size = { width, height }; }
      isDestroyed() { return this.destroyed; }
      destroy() { this.destroyed = true; }
    }

    const service = createPdfThumbnailService({
      BrowserWindow: FakeBrowserWindow,
      app: { getAppPath: () => '/unused' },
      path,
      isDev: true,
      devUrl: 'http://127.0.0.1:8080',
    });
    await service.render({
      sourceUrl: 'media://localhost/file/example.pdf',
      outputPath,
    });

    expect(windows[0].options.show).toBe(false);
    expect(windows[0].url).toContain('pdf-thumbnail.html?src=media%3A%2F%2Flocalhost%2Ffile%2Fexample.pdf');
    expect(windows[0].size).toEqual({ width: 400, height: 600 });
    expect(windows[0].captureRect).toEqual({ x: 0, y: 0, width: 400, height: 600 });
    expect(windows[0].destroyed).toBe(true);
    expect(await fs.readFile(outputPath, 'utf8')).toBe('pdf-page');
  });
});
