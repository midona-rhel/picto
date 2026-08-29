import { EventEmitter } from 'node:events';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createAutoUpdaterService, isMissingUpdateMetadataError } from './autoUpdater.mjs';

vi.mock('electron-updater', () => ({ default: { autoUpdater: {} } }));

class FakeUpdater extends EventEmitter {
  constructor(error) {
    super();
    this.error = error;
  }

  async checkForUpdates() {
    this.emit('error', this.error);
    throw this.error;
  }

  async downloadUpdate() {}

  quitAndInstall() {}
}

function createService(error, sendToAllWindows = vi.fn()) {
  const updater = new FakeUpdater(error);
  const service = createAutoUpdaterService({
    app: { getVersion: () => '0.6.0-alpha' },
    isDev: false,
    sendToAllWindows,
    updater,
  });
  return { service, updater, sendToAllWindows };
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('auto updater errors', () => {
  it('treats an unpublished platform feed as no available update', async () => {
    const error = new Error('Cannot find latest-mac.yml in the latest release artifacts: HttpError: 404');
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    const { service, updater, sendToAllWindows } = createService(error);

    await service.checkAndUpdateOnStartup(10);

    expect(isMissingUpdateMetadataError(error)).toBe(true);
    expect(updater.logger).toBeNull();
    expect(consoleError).not.toHaveBeenCalled();
    expect(sendToAllWindows).not.toHaveBeenCalledWith(
      'updater:status',
      expect.objectContaining({ status: 'error' }),
    );
  });

  it('reports a genuine updater failure only once', async () => {
    const error = new Error('Update signature is invalid');
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {});
    const { service, sendToAllWindows } = createService(error);

    await service.checkAndUpdateOnStartup(10);

    expect(consoleError).toHaveBeenCalledOnce();
    expect(sendToAllWindows).toHaveBeenCalledOnce();
    expect(sendToAllWindows).toHaveBeenCalledWith('updater:status', {
      status: 'error',
      error: 'Update signature is invalid',
    });
  });
});
