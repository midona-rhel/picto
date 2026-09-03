import { afterEach, describe, expect, test, vi } from 'vitest';
import { EventEmitter } from 'node:events';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { createUpdateService } from './updateService.mjs';

const tempDirectories = [];

function app(packaged = true, version = '0.6.0-alpha', userData = os.tmpdir()) {
  return { isPackaged: packaged, getVersion: () => version, getPath: () => userData };
}

function releases(...entries) {
  return { ok: true, json: async () => entries };
}

describe('update service', () => {
  afterEach(async () => {
    await Promise.all(tempDirectories.splice(0).map((directory) => fs.rm(directory, { recursive: true, force: true })));
  });

  test('does not contact update servers in development', async () => {
    const fetch = vi.fn();
    const service = createUpdateService({ app: app(false), net: { fetch }, sendToAllWindows: vi.fn(), platform: 'darwin' });
    expect((await service.check()).status).toBe('unavailable');
    expect(fetch).not.toHaveBeenCalled();
  });

  test('finds prerelease updates on macOS and preserves their release notes', async () => {
    const sendToAllWindows = vi.fn();
    const fetch = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => [{
        draft: false,
        tag_name: 'v0.6.1-alpha',
        name: 'Picto 0.6.1 Alpha',
        body: '- Faster imports',
        published_at: '2026-08-30T00:00:00Z',
        html_url: 'https://example.test/release',
      }],
    });
    const service = createUpdateService({ app: app(), net: { fetch }, sendToAllWindows, platform: 'darwin' });
    const state = await service.check();
    expect(state).toMatchObject({ status: 'available', version: '0.6.1-alpha', releaseNotes: '- Faster imports' });
    expect(sendToAllWindows).toHaveBeenLastCalledWith('picto:update-state', expect.objectContaining({ status: 'available' }));
  });

  test('reports the current version when no newer Mac release exists', async () => {
    const fetch = vi.fn().mockResolvedValue(releases({ draft: false, tag_name: 'v0.6.0-alpha' }));
    const service = createUpdateService({ app: app(), net: { fetch }, sendToAllWindows: vi.fn(), platform: 'darwin' });
    expect((await service.check()).status).toBe('current');
  });

  test('ignores non-application releases when checking for updates', async () => {
    const fetch = vi.fn().mockResolvedValue(releases(
      { draft: false, tag_name: 'v0.6.0-alpha' },
      { draft: false, tag_name: 'ai-models-v2', name: 'Picto AI Models v2' },
    ));
    const service = createUpdateService({ app: app(), net: { fetch }, sendToAllWindows: vi.fn(), platform: 'darwin' });

    expect(await service.check()).toMatchObject({ status: 'current', error: null });
  });

  test('loads the packaged updater from its CommonJS default export and pins it to the selected app release', async () => {
    const autoUpdater = Object.assign(new EventEmitter(), {
      checkForUpdates: vi.fn().mockResolvedValue(undefined),
      setFeedURL: vi.fn(),
    });
    const service = createUpdateService({
      app: app(),
      net: { fetch: vi.fn().mockResolvedValue(releases({ draft: false, tag_name: 'v0.6.1-alpha' })) },
      sendToAllWindows: vi.fn(),
      platform: 'win32',
      loadUpdaterModule: async () => ({ default: { autoUpdater } }),
    });

    await service.check();

    expect(autoUpdater.autoDownload).toBe(true);
    expect(autoUpdater.autoInstallOnAppQuit).toBe(true);
    expect(autoUpdater.disableDifferentialDownload).toBe(true);
    expect(autoUpdater.allowPrerelease).toBe(true);
    expect(autoUpdater.channel).toBe('latest');
    expect(autoUpdater.setFeedURL).toHaveBeenCalledWith({
      provider: 'generic',
      url: 'https://github.com/midona-rhel/picto/releases/download/v0.6.1-alpha/',
      channel: 'latest',
    });
    expect(autoUpdater.checkForUpdates).toHaveBeenCalledOnce();
  });

  test('publishes Windows availability before background download progress', async () => {
    const autoUpdater = Object.assign(new EventEmitter(), {
      checkForUpdates: vi.fn().mockImplementation(async () => {
        autoUpdater.emit('update-available', { version: '0.6.1-alpha' });
      }),
      setFeedURL: vi.fn(),
    });
    const sendToAllWindows = vi.fn();
    const service = createUpdateService({
      app: app(),
      net: { fetch: vi.fn().mockResolvedValue(releases({ draft: false, tag_name: 'v0.6.1-alpha' })) },
      sendToAllWindows,
      platform: 'win32',
      loadUpdaterModule: async () => ({ autoUpdater }),
    });

    expect(await service.check()).toMatchObject({ status: 'available', version: '0.6.1-alpha' });
    expect(sendToAllWindows).toHaveBeenLastCalledWith(
      'picto:update-state',
      expect.objectContaining({ status: 'available', version: '0.6.1-alpha' }),
    );
  });

  test('reports a stable error when the packaged updater cannot be loaded', async () => {
    const service = createUpdateService({
      app: app(),
      net: { fetch: vi.fn().mockResolvedValue(releases({ draft: false, tag_name: 'v0.6.1-alpha' })) },
      sendToAllWindows: vi.fn(),
      platform: 'win32',
      loadUpdaterModule: async () => ({}),
    });

    expect(await service.check()).toMatchObject({
      status: 'error',
      error: 'The packaged update service is unavailable.',
    });
  });

  test('restores installed release notes once after an update restart', async () => {
    const userData = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-update-'));
    tempDirectories.push(userData);
    await fs.writeFile(path.join(userData, 'pending-update-release.json'), JSON.stringify({
      version: '0.6.1-alpha',
      releaseName: 'Picto 0.6.1 Alpha',
      releaseNotes: '* Smoother grid scrolling',
      releaseUrl: 'https://example.test/release',
    }));
    const sendToAllWindows = vi.fn();
    const service = createUpdateService({
      app: app(true, '0.6.1-alpha', userData),
      net: { fetch: vi.fn() },
      sendToAllWindows,
      platform: 'win32',
    });

    await service.start();

    expect(service.getState()).toMatchObject({
      status: 'installed',
      version: '0.6.1-alpha',
      releaseNotes: '* Smoother grid scrolling',
    });
    expect(sendToAllWindows).toHaveBeenLastCalledWith(
      'picto:update-state',
      expect.objectContaining({ status: 'installed', version: '0.6.1-alpha' }),
    );
    expect(await service.check()).toMatchObject({ status: 'installed' });

    await service.acknowledgeInstalled();

    expect(service.getState()).toMatchObject({ status: 'current', version: null });
    await expect(fs.stat(path.join(userData, 'pending-update-release.json'))).rejects.toMatchObject({ code: 'ENOENT' });
  });

  test('persists downloaded release notes for the next packaged launch', async () => {
    const userData = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-update-'));
    tempDirectories.push(userData);
    const autoUpdater = Object.assign(new EventEmitter(), {
      checkForUpdates: vi.fn().mockImplementation(async () => {
        autoUpdater.emit('update-downloaded', {
          version: '0.6.1-alpha',
          releaseName: 'Picto 0.6.1 Alpha',
          releaseNotes: '* More reliable playback',
          downloadedFile: 'Picto-Setup.exe',
        });
      }),
      setFeedURL: vi.fn(),
    });
    const service = createUpdateService({
      app: app(true, '0.6.0-alpha', userData),
      net: { fetch: vi.fn().mockResolvedValue(releases({
        draft: false,
        tag_name: 'v0.6.1-alpha',
        body: '* More reliable playback',
      })) },
      sendToAllWindows: vi.fn(),
      platform: 'win32',
      loadUpdaterModule: async () => ({ autoUpdater }),
    });

    await service.check();

    await vi.waitFor(async () => {
      const pending = JSON.parse(await fs.readFile(path.join(userData, 'pending-update-release.json'), 'utf8'));
      expect(pending).toMatchObject({
        version: '0.6.1-alpha',
        releaseNotes: '* More reliable playback',
      });
    });
  });
});
