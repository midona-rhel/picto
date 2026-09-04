import { afterEach, describe, expect, test, vi } from 'vitest';
import { EventEmitter } from 'node:events';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { createUpdateService } from './updateService.mjs';

const tempDirectories = [];

function app(packaged = true, version = '0.6.0-alpha', userData = os.tmpdir()) {
  return { isPackaged: packaged, getVersion: () => version, getPath: () => userData, getAppPath: () => userData };
}

function releases(...entries) {
  return { ok: true, json: async () => entries };
}

describe('update service', () => {
  afterEach(async () => {
    vi.restoreAllMocks();
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

  test.each(['win32', 'darwin', 'linux'])('shows bundled notes offline on %s and remembers dismissal across restarts', async (platform) => {
    const userData = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-update-'));
    tempDirectories.push(userData);
    await fs.mkdir(path.join(userData, 'docs'));
    await fs.writeFile(path.join(userData, 'docs', '0.6.1-alpha-release-notes.md'), '* Smoother grid scrolling');
    const fetch = vi.fn().mockRejectedValue(new Error('offline'));
    const sendToAllWindows = vi.fn();
    const service = createUpdateService({
      app: app(true, '0.6.1-alpha', userData),
      net: { fetch },
      sendToAllWindows,
      platform,
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
    expect(JSON.parse(await fs.readFile(path.join(userData, 'acknowledged-update-release.json'), 'utf8'))).toEqual({ version: '0.6.1-alpha' });
    const restarted = createUpdateService({ app: app(true, '0.6.1-alpha', userData), net: { fetch }, sendToAllWindows: vi.fn(), platform });
    await restarted.start();
    expect(restarted.getState().status).not.toBe('installed');
    expect(fetch).not.toHaveBeenCalled();
    await fs.writeFile(path.join(userData, 'docs', '0.6.2-alpha-release-notes.md'), '* Better playback');
    const upgraded = createUpdateService({ app: app(true, '0.6.2-alpha', userData), net: { fetch }, sendToAllWindows: vi.fn(), platform });
    await upgraded.start();
    expect(upgraded.getState()).toMatchObject({ status: 'installed', version: '0.6.2-alpha', releaseNotes: '* Better playback' });
  });

  test('does not lose notes if the app quits before acknowledgement', async () => {
    const userData = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-update-'));
    tempDirectories.push(userData);
    await fs.mkdir(path.join(userData, 'docs'));
    await fs.writeFile(path.join(userData, 'docs', '0.6.1-alpha-release-notes.md'), '* Better playback');
    for (let launch = 0; launch < 2; launch++) {
      const service = createUpdateService({ app: app(true, '0.6.1-alpha', userData), net: { fetch: vi.fn() }, sendToAllWindows: vi.fn() });
      await service.start();
      expect(service.getState()).toMatchObject({ status: 'installed', releaseNotes: '* Better playback' });
    }
  });

  test('reports missing packaged notes without an unhandled startup rejection', async () => {
    const userData = await fs.mkdtemp(path.join(os.tmpdir(), 'picto-update-'));
    tempDirectories.push(userData);
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    const service = createUpdateService({ app: app(true, '0.6.1-alpha', userData), net: { fetch: vi.fn() }, sendToAllWindows: vi.fn() });
    await service.start();
    expect(service.getState().status).toBe('error');
    expect(error).toHaveBeenCalledWith(expect.stringContaining('installed release notes'), expect.any(Error));
  });
});
