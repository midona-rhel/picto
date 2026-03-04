import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Sidebar } from '../Sidebar';
import { imageDrag } from '../../../lib/imageDrag';
import { useDomainStore } from '../../../stores/domainStore';
import { useNavigationStore } from '../../../stores/navigationStore';
import { useCacheStore } from '../../../stores/cacheStore';

const {
  setStatusSelectionMock,
  invalidateSummaryMock,
  sidebarGetTreeMock,
  tagsNamespaceSummaryMock,
  gridGetPageSlimMock,
} = vi.hoisted(() => ({
  setStatusSelectionMock: vi.fn(),
  invalidateSummaryMock: vi.fn(),
  sidebarGetTreeMock: vi.fn(),
  tagsNamespaceSummaryMock: vi.fn(),
  gridGetPageSlimMock: vi.fn(),
}));

vi.mock('#desktop/api', () => ({
  api: {
    file: {
      setStatusSelection: setStatusSelectionMock,
    },
    sidebar: {
      getTree: sidebarGetTreeMock,
    },
    tags: {
      getNamespaceSummary: tagsNamespaceSummaryMock,
    },
    grid: {
      getPageSlim: gridGetPageSlimMock,
    },
  },
}));

vi.mock('../../../controllers/selectionController', () => ({
  SelectionController: {
    invalidateSummary: invalidateSummaryMock,
  },
}));

vi.mock('../FolderTree', () => ({
  FolderTree: () => <div data-testid="folder-tree" />,
}));

vi.mock('../SmartFolderList', () => ({
  SmartFolderList: () => <div data-testid="smart-folder-list" />,
}));

vi.mock('../LibrarySwitcher', () => ({
  LibrarySwitcher: () => <div data-testid="library-switcher" />,
}));

vi.mock('../../layout/SidebarJobStatus', () => ({
  SidebarJobStatus: () => <div data-testid="sidebar-job-status" />,
}));

function dispatchInternalDrop(label: string, hashes: string[]) {
  imageDrag.clearNativeDragSession();
  imageDrag.startNativeDragSession(hashes);
  const row = screen.getByText(label).closest('div');
  if (!row) throw new Error(`Sidebar row not found for "${label}"`);
  const dataTransfer = { dropEffect: 'none' } as unknown as DataTransfer;
  fireEvent.dragOver(row, { dataTransfer });
  fireEvent.drop(row, { dataTransfer });
}

describe('Sidebar drag-drop status targets', () => {
  beforeEach(() => {
    setStatusSelectionMock.mockReset();
    invalidateSummaryMock.mockReset();
    sidebarGetTreeMock.mockReset();
    tagsNamespaceSummaryMock.mockReset();
    gridGetPageSlimMock.mockReset();
    useDomainStore.setState({
      allImagesCount: 100,
      inboxCount: 20,
      trashCount: 5,
      untaggedCount: 10,
      recentViewedCount: 0,
      duplicatesCount: 0,
      folderNodes: [],
      smartFolders: [],
      smartFolderCounts: {},
      sidebarNodes: [],
      treeEpoch: 0,
      loading: false,
    });
    useNavigationStore.setState({
      currentView: 'images',
      activeSmartFolder: null,
      activeFolder: null,
      activeCollection: null,
      activeFlow: null,
      activeStatusFilter: null,
      filterTags: null,
    });
    useCacheStore.setState({ gridRefreshSeq: 0, metadataCache: new Map() });
    setStatusSelectionMock.mockResolvedValue(0);
    sidebarGetTreeMock.mockResolvedValue({ nodes: [], tree_epoch: 1, generated_at: new Date(0).toISOString() });
    tagsNamespaceSummaryMock.mockResolvedValue([]);
    gridGetPageSlimMock.mockResolvedValue({ items: [], total_count: 0, next_cursor: null });
  });

  afterEach(() => {
    imageDrag.clearNativeDragSession();
  });

  it('drops to All Images and restores active status', async () => {
    render(<Sidebar />);
    dispatchInternalDrop('All Images', ['hash_a', 'hash_b']);

    expect(setStatusSelectionMock).toHaveBeenCalledWith(
      { mode: 'explicit_hashes', hashes: ['hash_a', 'hash_b'] },
      'active',
    );
    await waitFor(() => expect(invalidateSummaryMock).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(useCacheStore.getState().gridRefreshSeq).toBe(1));
  });

  it('drops to Inbox and sets inbox status', async () => {
    render(<Sidebar />);
    dispatchInternalDrop('Inbox', ['hash_x']);

    expect(setStatusSelectionMock).toHaveBeenCalledWith(
      { mode: 'explicit_hashes', hashes: ['hash_x'] },
      'inbox',
    );
    await waitFor(() => expect(invalidateSummaryMock).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(useCacheStore.getState().gridRefreshSeq).toBe(1));
  });

  it('drops to Trash and sets trash status', async () => {
    render(<Sidebar />);
    dispatchInternalDrop('Trash', ['hash_z']);

    expect(setStatusSelectionMock).toHaveBeenCalledWith(
      { mode: 'explicit_hashes', hashes: ['hash_z'] },
      'trash',
    );
    await waitFor(() => expect(invalidateSummaryMock).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(useCacheStore.getState().gridRefreshSeq).toBe(1));
  });
});
