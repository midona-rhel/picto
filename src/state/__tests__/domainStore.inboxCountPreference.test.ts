import { beforeEach, describe, expect, it, vi } from 'vitest';

const { getTreeMock, getNamespaceSummaryMock, getPageSlimMock } = vi.hoisted(() => ({
  getTreeMock: vi.fn(),
  getNamespaceSummaryMock: vi.fn(),
  getPageSlimMock: vi.fn(),
}));

vi.mock('#desktop/api', () => ({
  api: {
    sidebar: {
      getTree: getTreeMock,
    },
    tags: {
      getNamespaceSummary: getNamespaceSummaryMock,
    },
    grid: {
      getPageSlim: getPageSlimMock,
    },
  },
}));

import { useDomainStore } from '../domainStore';

describe('domainStore inbox count resolution', () => {
  beforeEach(() => {
    getTreeMock.mockReset();
    getNamespaceSummaryMock.mockReset();
    getPageSlimMock.mockReset();

    useDomainStore.setState({
      allImagesCount: 0,
      inboxCount: 4,
      uncategorizedCount: 0,
      trashCount: 0,
      untaggedCount: 0,
      tagsCount: 0,
      recentViewedCount: 0,
      duplicatesCount: 0,
      smartFolders: [],
      smartFolderCounts: {},
      folderNodes: [],
      sidebarNodes: [],
      treeEpoch: 0,
      liveInboxImportRuns: 0,
      liveInboxFloor: null,
      loading: false,
    });

    getNamespaceSummaryMock.mockResolvedValue([]);
    getPageSlimMock.mockImplementation(async ({ status }: { status: string }) => {
      if (status === 'inbox') return { items: [], total_count: 4, next_cursor: null };
      return { items: [], total_count: 0, next_cursor: null };
    });
  });

  it('prefers sidebar tree inbox count over stale cached inbox grid totals', async () => {
    getTreeMock.mockResolvedValue({
      nodes: [
        { id: 'system:all', kind: 'system', name: 'All Images', count: 10 },
        { id: 'system:inbox', kind: 'system', name: 'Inbox', count: 7 },
        { id: 'system:trash', kind: 'system', name: 'Trash', count: 0 },
      ],
      tree_epoch: 2,
      generated_at: new Date(0).toISOString(),
    });

    await useDomainStore.getState().fetchSidebarTree();

    expect(useDomainStore.getState().inboxCount).toBe(7);
  });

  it('does not let a stale sidebar fetch lower live inbox count during subscription imports', async () => {
    useDomainStore.getState().subscriptionRunStarted();
    useDomainStore.getState().applySidebarCounts({
      all_images: 10,
      inbox: 7,
      trash: 0,
    });

    getTreeMock.mockResolvedValue({
      nodes: [
        { id: 'system:all', kind: 'system', name: 'All Images', count: 10 },
        { id: 'system:inbox', kind: 'system', name: 'Inbox', count: 4 },
        { id: 'system:trash', kind: 'system', name: 'Trash', count: 0 },
      ],
      tree_epoch: 2,
      generated_at: new Date(0).toISOString(),
    });

    await useDomainStore.getState().fetchSidebarTree();

    expect(useDomainStore.getState().inboxCount).toBe(7);
  });

  it('allows inbox count to decrease again after subscription imports finish', async () => {
    useDomainStore.getState().subscriptionRunStarted();
    useDomainStore.getState().applySidebarCounts({
      all_images: 10,
      inbox: 7,
      trash: 0,
    });
    useDomainStore.getState().subscriptionRunFinished();

    getTreeMock.mockResolvedValue({
      nodes: [
        { id: 'system:all', kind: 'system', name: 'All Images', count: 10 },
        { id: 'system:inbox', kind: 'system', name: 'Inbox', count: 4 },
        { id: 'system:trash', kind: 'system', name: 'Trash', count: 0 },
      ],
      tree_epoch: 3,
      generated_at: new Date(0).toISOString(),
    });

    await useDomainStore.getState().fetchSidebarTree();

    expect(useDomainStore.getState().inboxCount).toBe(4);
  });
});
