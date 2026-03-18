import { fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Sidebar } from '../Sidebar';
import { imageDrag } from '../../../../shared/lib/imageDrag';
import { useDomainStore } from '../../../../state/domainStore';
import { useNavigationStore } from '../../../../state/navigationStore';
import { useGridMetadataStore } from '../../../../state/gridMetadataStore';

const {
  setStatusSelectionMock,
  sidebarGetTreeMock,
  tagsNamespaceSummaryMock,
  gridGetPageSlimMock,
} = vi.hoisted(() => ({
  setStatusSelectionMock: vi.fn(),
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

vi.mock('../FolderTree', () => ({
  FolderTree: () => <div data-testid="folder-tree" />,
}));

vi.mock('../SmartFolderList', () => ({
  SmartFolderList: () => <div data-testid="smart-folder-list" />,
}));

vi.mock('../LibrarySwitcher', () => ({
  LibrarySwitcher: () => <div data-testid="library-switcher" />,
}));

vi.mock('../../../layout/components/SidebarJobStatus', () => ({
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
    sidebarGetTreeMock.mockReset();
    tagsNamespaceSummaryMock.mockReset();
    gridGetPageSlimMock.mockReset();
    useDomainStore.setState({
      allActiveCount: 100,
      inboxCount: 20,
      uncategorizedCount: 12,
      trashCount: 5,
      untaggedCount: 10,
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
      activeSmartFolderId: null,
      activeFolder: null,
      activeCollection: null,
      activeSubscriptionGroup: null,
      activeStatusFilter: null,
      filterTags: null,
    });
    useGridMetadataStore.setState({ gridRefreshSeq: 0, metadataCache: new Map() });
    setStatusSelectionMock.mockResolvedValue(0);
    sidebarGetTreeMock.mockResolvedValue({ nodes: [], tree_epoch: 1, generated_at: new Date(0).toISOString() });
    tagsNamespaceSummaryMock.mockResolvedValue([]);
    gridGetPageSlimMock.mockResolvedValue({ items: [], total_count: 0, next_cursor: null });
  });

  afterEach(() => {
    imageDrag.clearNativeDragSession();
  });

  it('drops to All Active and restores active status', async () => {
    render(<Sidebar />);
    dispatchInternalDrop('All Active', ['hash_a', 'hash_b']);

    expect(setStatusSelectionMock).toHaveBeenCalledWith(
      { mode: 'explicit_hashes', hashes: ['hash_a', 'hash_b'] },
      'active',
    );
  });

  it('drops to Inbox and sets inbox status', async () => {
    render(<Sidebar />);
    dispatchInternalDrop('Inbox', ['hash_x']);

    expect(setStatusSelectionMock).toHaveBeenCalledWith(
      { mode: 'explicit_hashes', hashes: ['hash_x'] },
      'inbox',
    );
  });

  it('drops to Trash and sets trash status', async () => {
    render(<Sidebar />);
    dispatchInternalDrop('Trash', ['hash_z']);

    expect(setStatusSelectionMock).toHaveBeenCalledWith(
      { mode: 'explicit_hashes', hashes: ['hash_z'] },
      'trash',
    );
  });
});
