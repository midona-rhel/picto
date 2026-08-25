import { afterEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => {
  let callback: (() => void) | undefined;
  return {
    invoke: vi.fn(),
    register: vi.fn((_resource: string, next: () => void) => {
      callback = next;
      return () => { callback = undefined; };
    }),
    callback: () => callback,
    info: vi.fn(),
    success: vi.fn(),
    error: vi.fn(),
  };
});

vi.mock('../platform/ipc', () => ({ invoke: mocks.invoke }));
vi.mock('./libraryInvalidation', () => ({
  libraryInvalidation: { register: mocks.register },
}));
vi.mock('../shared/lib/notifications', () => ({
  showInfoNotification: mocks.info,
  showSuccessNotification: mocks.success,
  showErrorNotification: mocks.error,
}));

import { startCloudSettle } from './cloudSettle';

const idle = {
  state: 'idle', phase: 'idle', blocking: false, completed_units: 0,
  total_units: null, message: '', last_sync_at: null,
  pending_mutations: 0, pending_blobs: 0, missing_blobs: 0,
};

async function settle() {
  await vi.waitFor(() => expect(mocks.register).toHaveBeenCalled());
  await new Promise((resolve) => setTimeout(resolve, 0));
}

describe('cloud settlement', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.clearAllMocks();
  });

  it('uses standard notifications for reconnect progress and long completion', async () => {
    const now = vi.spyOn(Date, 'now');
    now.mockReturnValue(1_000);
    mocks.invoke.mockResolvedValueOnce(idle);
    const stop = startCloudSettle();
    await settle();

    mocks.invoke.mockResolvedValueOnce({ ...idle, state: 'reconciling', phase: 'downloading' });
    mocks.callback()?.();
    await vi.waitFor(() => expect(mocks.info).toHaveBeenCalledOnce());
    expect(mocks.register).toHaveBeenCalledWith('cloud', expect.any(Function));

    now.mockReturnValue(7_000);
    mocks.invoke.mockResolvedValueOnce(idle);
    mocks.callback()?.();
    await vi.waitFor(() => expect(mocks.success).toHaveBeenCalledWith({
      title: 'Cloud update complete',
      message: 'Your library is up to date.',
    }));
    stop();
  });

  it('reports persisted cloud failures through the same notification host', async () => {
    mocks.invoke.mockResolvedValueOnce(idle);
    const stop = startCloudSettle();
    await settle();

    mocks.invoke.mockResolvedValueOnce({ ...idle, state: 'error', message: 'Snapshot checksum failed' });
    mocks.callback()?.();
    await vi.waitFor(() => expect(mocks.error).toHaveBeenCalledWith(expect.objectContaining({
      title: 'Cloud sync needs attention',
      message: 'Snapshot checksum failed',
    })));
    stop();
  });
});
