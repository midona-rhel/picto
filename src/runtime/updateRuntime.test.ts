import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';

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
import { updateModalAtom } from '../state/modals';

describe('update runtime', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    getDefaultStore().set(updateModalAtom, { open: false });
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

  it('does not open notes from a disposed startup or leak its delayed listener', async () => {
    let finishState!: (state: unknown) => void;
    let finishListener!: (dispose: () => void) => void;
    mocks.getState.mockReturnValue(new Promise((resolve) => { finishState = resolve; }));
    mocks.onState.mockReturnValue(new Promise((resolve) => { finishListener = resolve; }));
    const stop = startUpdateRuntime();
    stop();
    const unsubscribe = vi.fn();
    finishListener(unsubscribe);
    finishState({ status: 'installed', version: '0.6.12-alpha' });
    await vi.waitFor(() => expect(unsubscribe).toHaveBeenCalledOnce());
    expect(getDefaultStore().get(updateModalAtom)).toEqual({ open: false });
  });

  it('does not reopen acknowledged notes from a stale initial read', async () => {
    let finishState!: (state: unknown) => void;
    let onState!: (state: unknown) => void;
    mocks.getState.mockReturnValue(new Promise((resolve) => { finishState = resolve; }));
    mocks.onState.mockImplementation((listener) => { onState = listener; return Promise.resolve(vi.fn()); });
    const stop = startUpdateRuntime();
    onState({ status: 'current', version: null });
    finishState({ status: 'installed', version: '0.6.12-alpha' });
    await Promise.resolve();
    expect(getDefaultStore().get(updateModalAtom)).toEqual({ open: false });
    stop();
  });

  it('opens user-facing release notes after an installed update restarts', async () => {
    mocks.getState.mockResolvedValue({
      status: 'installed',
      version: '0.6.11-alpha',
      platform: 'win32',
      releaseNotes: '* Improved grid performance',
    });

    startUpdateRuntime();

    await vi.waitFor(() => expect(getDefaultStore().get(updateModalAtom)).toEqual({ open: true }));
    expect(mocks.showInfo).not.toHaveBeenCalled();
    expect(mocks.showSuccess).not.toHaveBeenCalled();
  });
});
