import { beforeEach, describe, expect, it, vi } from 'vitest';
import { setupEventBridge, teardownEventBridge } from '../eventBridge';

const { listeners, cacheState } = vi.hoisted(() => ({
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  cacheState: {
    activeGridScope: 'system:inbox' as string | null,
    invalidateAll: vi.fn(),
    bumpGridRefresh: vi.fn(),
    setActiveGridScope: vi.fn(),
    clearInvalidatedHashes: vi.fn(),
    metadataInvalidatedHashes: new Set<string>(),
    invalidateHash: vi.fn(),
    markHashInvalidated: vi.fn(),
  },
}));

vi.mock('#desktop/api', () => ({
  listen: vi.fn((name: string, handler: (event: { payload: unknown }) => void) => {
    listeners.set(name, handler);
    return Promise.resolve(() => {
      listeners.delete(name);
    });
  }),
}));

vi.mock('../cacheStore', () => ({
  useCacheStore: {
    getState: () => cacheState,
  },
}));

vi.mock('../domainStore', () => ({
  useDomainStore: {
    getState: () => ({
      applySidebarCounts: vi.fn(),
      subscriptionRunStarted: vi.fn(),
      subscriptionRunFinished: vi.fn(),
    }),
  },
}));

vi.mock('../libraryStore', () => ({
  useLibraryStore: {
    getState: () => ({
      setSwitching: vi.fn(),
      loadConfig: vi.fn(),
    }),
  },
}));

vi.mock('../../controllers/sidebarController', () => ({
  SidebarController: {
    requestRefresh: vi.fn(),
  },
}));

vi.mock('../../controllers/selectionController', () => ({
  SelectionController: {
    invalidateSummary: vi.fn(),
  },
}));

vi.mock('../../components/image-grid/metadataPrefetch', () => ({
  invalidateMetadata: vi.fn(),
}));

describe('eventBridge inbox subscription import behavior', () => {
  beforeEach(() => {
    listeners.clear();
    cacheState.activeGridScope = 'system:inbox';
    cacheState.invalidateAll.mockReset();
    cacheState.bumpGridRefresh.mockReset();
    teardownEventBridge();
  });

  it('skips inbox hard-reload for subscription_import invalidations', async () => {
    await setupEventBridge();
    const onStateChanged = listeners.get('state-changed');
    expect(onStateChanged).toBeTruthy();

    onStateChanged!({
      payload: {
        seq: 101,
        origin_command: 'subscription_import',
        invalidate: { grid_scopes: ['system:all', 'system:inbox'] },
      },
    });

    expect(cacheState.invalidateAll).not.toHaveBeenCalled();
    expect(cacheState.bumpGridRefresh).not.toHaveBeenCalled();
  });

  it('still reloads inbox for non-subscription grid invalidations', async () => {
    await setupEventBridge();
    const onStateChanged = listeners.get('state-changed');
    expect(onStateChanged).toBeTruthy();

    onStateChanged!({
      payload: {
        seq: 102,
        origin_command: 'set_status_selection',
        invalidate: { grid_scopes: ['system:all', 'system:inbox'] },
      },
    });

    expect(cacheState.invalidateAll).toHaveBeenCalledTimes(1);
    expect(cacheState.bumpGridRefresh).toHaveBeenCalledTimes(1);
  });
});
