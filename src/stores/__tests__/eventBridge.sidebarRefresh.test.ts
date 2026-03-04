import { beforeEach, describe, expect, it, vi } from 'vitest';
import { setupEventBridge, teardownEventBridge } from '../eventBridge';

const { listeners, requestRefreshMock } = vi.hoisted(() => ({
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  requestRefreshMock: vi.fn(),
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
    getState: () => ({
      activeGridScope: null,
      invalidateAll: vi.fn(),
      bumpGridRefresh: vi.fn(),
      setActiveGridScope: vi.fn(),
      clearInvalidatedHashes: vi.fn(),
      metadataInvalidatedHashes: new Set<string>(),
      invalidateHash: vi.fn(),
      markHashInvalidated: vi.fn(),
    }),
  },
}));

vi.mock('../domainStore', () => ({
  useDomainStore: {
    getState: () => ({
      applySidebarCounts: vi.fn(),
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
    requestRefresh: requestRefreshMock,
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

describe('eventBridge sidebar refresh ownership', () => {
  beforeEach(() => {
    listeners.clear();
    requestRefreshMock.mockReset();
    teardownEventBridge();
  });

  it('emits one sidebar refresh for one mutation seq', async () => {
    await setupEventBridge();
    const onStateChanged = listeners.get('state-changed');
    const onSidebarInvalidated = listeners.get('sidebar-invalidated');
    expect(onStateChanged).toBeTruthy();
    expect(onSidebarInvalidated).toBeTruthy();

    onStateChanged!({
      payload: {
        seq: 101,
        invalidate: { sidebar_tree: true },
      },
    });
    onSidebarInvalidated!({ payload: { seq: 101 } });

    expect(requestRefreshMock).toHaveBeenCalledTimes(1);
  });
});
