import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  getState: vi.fn(),
  onState: vi.fn(),
  showInfo: vi.fn(),
  showSuccess: vi.fn(),
}));

vi.mock('../platform/updateApi', () => ({
  getUpdateState: mocks.getState,
  onUpdateState: mocks.onState,
}));
vi.mock('../shared/lib/notifications', () => ({
  showInfoNotification: mocks.showInfo,
  showSuccessNotification: mocks.showSuccess,
}));

import { startUpdateRuntime } from './updateRuntime';

describe('update runtime', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.getState.mockResolvedValue({ status: 'current', version: null });
    mocks.onState.mockResolvedValue(vi.fn());
  });

  it('announces an available update in the Windows main window', async () => {
    mocks.getState.mockResolvedValue({
      status: 'available',
      version: '0.6.9-alpha',
      platform: 'win32',
    });

    startUpdateRuntime();

    await vi.waitFor(() => expect(mocks.showInfo).toHaveBeenCalledOnce());
    expect(mocks.showInfo).toHaveBeenCalledWith(expect.objectContaining({
      title: 'Picto 0.6.9-alpha is available',
      message: 'Updates download in the background and install after Picto closes.',
    }));
  });
});
