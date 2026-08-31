import { afterEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import { pictoPackModalAtom } from '../state/modals';

const mocks = vi.hoisted(() => ({
  claim: vi.fn<[], Promise<string | null>>(),
  open: vi.fn<[string], Promise<void>>(),
  listeners: new Map<string, () => void>(),
  showError: vi.fn(),
}));

vi.mock('../platform/associatedFilesApi', () => ({ claimAssociatedPictoPack: mocks.claim }));
vi.mock('../controllers/filesController', () => ({ openPictoPackImport: mocks.open }));
vi.mock('../platform/ipc', () => ({
  listen: vi.fn((name: string, handler: () => void) => {
    mocks.listeners.set(name, handler);
    return Promise.resolve(vi.fn());
  }),
}));
vi.mock('../shared/lib/notifications', () => ({ showErrorNotification: mocks.showError }));

import { startAssociatedFilesRuntime } from './associatedFilesRuntime';

const store = getDefaultStore();

afterEach(() => {
  store.set(pictoPackModalAtom, { open: false });
  mocks.listeners.clear();
  vi.clearAllMocks();
});

describe('associated files runtime', () => {
  it('claims a launched pack and opens the canonical import preview', async () => {
    mocks.claim.mockResolvedValueOnce('/tmp/portfolio.picto-pack').mockResolvedValue(null);
    mocks.open.mockResolvedValue(undefined);
    const stop = startAssociatedFilesRuntime();

    await vi.waitFor(() => expect(mocks.open).toHaveBeenCalledWith('/tmp/portfolio.picto-pack'));
    stop();
  });

  it('waits until the current pack modal closes before claiming another file', async () => {
    store.set(pictoPackModalAtom, {
      open: true,
      mode: 'export',
      source: { kind: 'items', target: { kind: 'explicit', root_ids: [7] } },
      itemCount: 1,
      suggestedName: 'One',
    });
    mocks.claim.mockResolvedValueOnce('/tmp/next.picto-pack').mockResolvedValue(null);
    mocks.open.mockResolvedValue(undefined);
    const stop = startAssociatedFilesRuntime();
    await Promise.resolve();
    expect(mocks.claim).not.toHaveBeenCalled();

    store.set(pictoPackModalAtom, { open: false });
    await vi.waitFor(() => expect(mocks.open).toHaveBeenCalledWith('/tmp/next.picto-pack'));
    stop();
  });
});
