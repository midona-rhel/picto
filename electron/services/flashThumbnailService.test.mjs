import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import {
  createFlashThumbnailService,
  fitSize,
  FLASH_THUMBNAIL_SETTLE_MS,
} from './flashThumbnailService.mjs';

const temporaryDirectories = [];

afterEach(async () => {
  await Promise.all(temporaryDirectories.splice(0).map((directory) => fs.rm(directory, { recursive: true, force: true })));
});

describe('flash thumbnail service', () => {
  it('uses a five-second representative-frame delay', () => {
    expect(FLASH_THUMBNAIL_SETTLE_MS).toBe(5_000);
  });

  it('fits the Flash stage without changing its aspect ratio', () => {
    expect(fitSize(400, 200, 800, 800)).toEqual({ width: 800, height: 400 });
    expect(fitSize(200, 400, 800, 800)).toEqual({ width: 400, height: 800 });
  });

  it('captures an isolated hidden stage to PNG', async () => {
    const directory = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-flash-thumbnail-'));
    temporaryDirectories.push(directory);
    const outputPath = path.join(directory, 'nested', 'thumbnail.png');
    const windows = [];

    class FakeBrowserWindow {
      constructor(options) {
        this.options = options;
        this.destroyed = false;
        this.webContents = {
          setAudioMuted: (muted) => { this.audioMuted = muted; },
          executeJavaScript: async () => ({ ready: true, width: 400, height: 200 }),
          capturePage: async () => ({ toPNG: () => Buffer.from('captured-png') }),
        };
        windows.push(this);
      }
      async loadURL(url) { this.url = url; }
      setContentSize(width, height) { this.size = { width, height }; }
      isDestroyed() { return this.destroyed; }
      destroy() { this.destroyed = true; }
    }

    const service = createFlashThumbnailService({
      BrowserWindow: FakeBrowserWindow,
      app: { getAppPath: () => '/unused' },
      path,
      isDev: true,
      devUrl: 'http://127.0.0.1:8080',
    });
    await service.render({
      sourceUrl: 'media://localhost/file/example.swf',
      outputPath,
      settleMs: 0,
    });

    expect(windows[0].options.show).toBe(false);
    expect(windows[0].options.show).toBe(false);
    expect(windows[0].audioMuted).toBe(true);
    expect(windows[0].url).toContain('flash-thumbnail.html?src=media%3A%2F%2Flocalhost%2Ffile%2Fexample.swf');
    expect(windows[0].size).toEqual({ width: 800, height: 400 });
    expect(windows[0].destroyed).toBe(true);
    expect(await fs.readFile(outputPath, 'utf8')).toBe('captured-png');
  });

});
