import { describe, expect, test, vi } from 'vitest';
import { EventEmitter } from 'node:events';
import { createUpdateService } from './updateService.mjs';

function app(packaged = true) {
  return { isPackaged: packaged, getVersion: () => '0.6.0-alpha' };
}

function releases(...entries) {
  return { ok: true, json: async () => entries };
}

describe('update service', () => {
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
});
