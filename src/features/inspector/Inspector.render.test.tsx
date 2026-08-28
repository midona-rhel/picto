import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { getDefaultStore } from 'jotai';
import { MantineProvider } from '@mantine/core';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const useAtomValue = vi.hoisted(() => vi.fn());

vi.mock('jotai', async (importOriginal) => ({
  ...(await importOriginal<typeof import('jotai')>()),
  useAtomValue,
}));

vi.mock('../../controllers/entityMutations', () => ({
  getTargetSelectionSummary: vi.fn(),
  removeEntityFromFolder: vi.fn(),
  removeEntityTags: vi.fn(),
  removeTargetTags: vi.fn(),
  removeItemFromFolder: vi.fn(),
  removeItemTags: vi.fn(),
  setItemName: vi.fn(),
  setItemNotes: vi.fn(),
  setItemRating: vi.fn(),
  setItemSourceUrls: vi.fn(),
  setEntityName: vi.fn(),
  setEntityNotes: vi.fn(),
  setEntityRating: vi.fn(),
  setEntitySourceUrls: vi.fn(),
  setTargetNotes: vi.fn(),
  setTargetRating: vi.fn(),
  setTargetSourceUrls: vi.fn(),
  updateTargetFolderMembership: vi.fn(),
}));

import { Inspector } from './Inspector';
import * as entityMutations from '../../controllers/entityMutations';
import {
  displayedInspectorItemDetailsAtom,
  displayedInspectorTargetAtom,
  inspectorErrorAtom,
  inspectorLoadingAtom,
  scopeInspectorViewModelAtom,
} from '../../state/inspector';
import { sidebarNodesAtom } from '../../state/sidebar';
import { gridItemsAtom } from '../../state/grid';
import {
  selectionCountAtom,
  selectionFingerprintAtom,
  selectionTargetAtom,
} from '../../state/selection';
import { folderPickerPortalAtom, tagSelectPortalAtom } from '../../state/portals';
import { confirmModalAtom, exportModalAtom } from '../../state/modals';

const entity = {
  item_id: 1, kind: 'media', lifecycle: 'active', label: 'Example', cover_media_item_id: null,
  folder_ids: [], aggregate_tags: [], revision: 1,
  media: [{ media_item_id: 1, file_hash: 'file-1', mime_type: 'image/jpeg', dominant_color_hex: '#123456', dominant_colors: ['#123456', '#abcdef'],
    size_bytes: 100, pixel_width: 20, pixel_height: 10, duration_ms: null, frame_count: null,
    name: 'Example', notes: null, rating: null, source_urls: [],
    captured_at: '2026-01-01', imported_at: '2026-01-02', position: 0, tags: [] }],
};

const group = {
  item_id: 10, kind: 'collection', lifecycle: 'active', label: 'Album', cover_media_item_id: 11,
  folder_ids: [7], aggregate_tags: ['general:member-tag'], revision: 1,
  media: [entity.media[0], { ...entity.media[0], media_item_id: 12, file_hash: 'file-2', mime_type: 'video/mp4', position: 1 }],
};

function renderInspector({
  target,
  data = null,
  scope = null,
  loading = false,
  error = null,
  selectionTarget = null,
  selectionCount,
  selectionFingerprint = 'test',
}: {
  target: unknown;
  data?: unknown;
  scope?: unknown;
  loading?: boolean;
  error?: string | null;
  selectionTarget?: unknown;
  selectionCount?: number;
  selectionFingerprint?: string;
}) {
  const values = new Map<unknown, unknown>([
    [displayedInspectorTargetAtom, target],
    [inspectorLoadingAtom, loading],
    [inspectorErrorAtom, error],
    [displayedInspectorItemDetailsAtom, data],
    [scopeInspectorViewModelAtom, scope],
    [sidebarNodesAtom, []],
    [gridItemsAtom, []],
    [selectionTargetAtom, selectionTarget],
    [selectionCountAtom, selectionCount ?? (target && (target as { kind?: string }).kind === 'multi' ? 2 : 0)],
    [selectionFingerprintAtom, selectionFingerprint],
  ]);
  useAtomValue.mockImplementation((atom) => values.get(atom));
  return render(<MantineProvider><Inspector /></MantineProvider>);
}

function assertStableAnchors(expectedCoreLabels: string[], expectedIdentityAnchors = ['name', 'notes', 'source']) {
  expect([...document.querySelectorAll('[data-inspector-anchor]')].map((node) => node.getAttribute('data-inspector-anchor')))
    .toEqual(expectedIdentityAnchors);
  expect([...document.querySelectorAll('[data-inspector-core-property]')].map((node) => node.getAttribute('data-inspector-core-property')))
    .toEqual(expectedCoreLabels);
}

function assertNoClassificationSections() {
  expect(document.querySelector('[data-inspector-section="tags"]')).not.toBeInTheDocument();
  expect(document.querySelector('[data-inspector-section="folders"]')).not.toBeInTheDocument();
  expect(document.querySelector('[data-inspector-empty-action]')).not.toBeInTheDocument();
}

describe('Inspector presentation branches', () => {
  beforeEach(() => {
    useAtomValue.mockReset();
    vi.mocked(entityMutations.getTargetSelectionSummary).mockReset();
    vi.mocked(entityMutations.setItemRating).mockReset();
    vi.mocked(entityMutations.setTargetNotes).mockReset();
    vi.mocked(entityMutations.setTargetRating).mockReset();
    vi.mocked(entityMutations.setTargetSourceUrls).mockReset();
    const store = getDefaultStore();
    store.set(tagSelectPortalAtom, { open: false, anchor: null });
    store.set(folderPickerPortalAtom, { open: false, anchor: null });
    store.set(confirmModalAtom, { open: false, title: '', message: '', onConfirm: () => {} });
    store.set(exportModalAtom, { open: false, fileCount: 0 });
  });

  it('uses item identity fields only when the target has item identity', () => {
    const cases = [
      { target: { kind: 'item', itemId: 1 }, data: entity, unavailableSource: false },
      { target: { kind: 'multi', count: 2, selectionMode: 'explicit' }, data: null, unavailableSource: true },
      { target: { kind: 'item', itemId: 1 }, data: null, loading: true, unavailableSource: true },
      { target: { kind: 'item', itemId: 1 }, data: null, error: 'Unavailable', unavailableSource: true },
    ];

    for (const entry of cases) {
      const view = renderInspector(entry);
      assertStableAnchors(
        entry.data ? ['Items', 'Dimensions', 'Size', 'Type', 'Date added', 'Date created'] : [],
        entry.target.kind === 'multi' ? ['notes', 'source'] : undefined,
      );
      const sections = [...document.querySelectorAll('[data-inspector-section]')].map((node) => node.getAttribute('data-inspector-section'));
      if (entry.data || entry.target.kind === 'multi') {
        expect(sections.slice(0, 3)).toEqual(['tags', 'folders', 'properties']);
      } else {
        expect(sections).toEqual(['properties']);
      }
      if (entry.unavailableSource && entry.target.kind !== 'multi') {
        expect(document.querySelector('[data-inspector-anchor="source"]')).toHaveTextContent('—');
      }
      if (entry.target.kind === 'multi') {
        expect(document.querySelector('[data-inspector-selection-count]')).toHaveTextContent('2 items selected');
      }
      view.unmount();
    }
  });

  it('shows only one identity-field hover popover at a time', () => {
    renderInspector({
      target: { kind: 'item', itemId: 1 },
      data: {
        ...entity,
        media: [{
          ...entity.media[0],
          notes: 'Inspector notes',
          source_urls: ['https://example.com/source'],
        }],
      },
    });

    const name = document.querySelector('[data-inspector-anchor="name"] > div')!;
    const notes = document.querySelector('[data-inspector-anchor="notes"] > div')!;
    const source = document.querySelector('[data-inspector-anchor="source"] > div')!;

    fireEvent.mouseEnter(name);
    expect(document.querySelectorAll('[data-inspector-field-popover]')).toHaveLength(1);
    expect(name).toHaveTextContent('Example');

    fireEvent.mouseEnter(notes);
    expect(document.querySelectorAll('[data-inspector-field-popover]')).toHaveLength(1);
    expect(notes).toHaveTextContent('Inspector notes');

    fireEvent.mouseEnter(source.querySelector('[class]') ?? source);
    expect(document.querySelectorAll('[data-inspector-field-popover]')).toHaveLength(1);
    expect(source).toHaveTextContent('example.com');
  });

  it('uses the shared inspector action primitive for Add Tags', () => {
    const view = renderInspector({ target: { kind: 'item', itemId: 1 }, data: entity });

    const addTags = document.querySelector('[data-inspector-action="add-tags"]');
    const autoTag = document.querySelector('[data-inspector-action="auto-tag"]');
    expect(addTags).toHaveTextContent('Add Tags');
    expect(addTags).toHaveAttribute('data-inspector-button-primitive', 'action');
    expect(autoTag).toHaveAttribute('data-inspector-button-primitive', 'action');
    view.unmount();
  });

  it('uses the canonical property row and commits one rating change', () => {
    const view = renderInspector({ target: { kind: 'item', itemId: 1 }, data: entity });

    const ratingLabel = screen.getByText('Rating');
    const itemsLabel = screen.getByText('Items');
    expect(ratingLabel.parentElement?.className).toBe(itemsLabel.parentElement?.className);

    fireEvent.click(screen.getByRole('button', { name: 'Set rating to 4 stars' }));
    expect(entityMutations.setItemRating).toHaveBeenCalledTimes(1);
    expect(entityMutations.setItemRating).toHaveBeenCalledWith(1, 4);
    view.unmount();
  });

  it('renders the complete persisted dominant-color palette', () => {
    const view = renderInspector({ target: { kind: 'item', itemId: 1 }, data: entity });

    expect(document.querySelector('[class*="previewFrame"]')).toHaveStyle({ background: '#123456' });
    const swatches = [...document.querySelectorAll('[class*="swatchWrap"]')];
    expect(swatches).toHaveLength(2);
    expect(swatches[0].querySelector('[class*="swatch"]')).toHaveStyle({ backgroundColor: '#123456' });
    expect(swatches[1].querySelector('[class*="swatch"]')).toHaveStyle({ backgroundColor: '#abcdef' });
    view.unmount();
  });

  it('shows the canonical file type label over a single-item preview', () => {
    const view = renderInspector({ target: { kind: 'item', itemId: 1 }, data: entity });

    expect(document.querySelector('[data-inspector-format-label]')).toHaveTextContent('JPG');
    view.unmount();
  });

  it('reuses the shared entity actions for a single-media preview', () => {
    const view = renderInspector({ target: { kind: 'item', itemId: 1 }, data: entity });

    fireEvent.contextMenu(document.querySelector('[class*="previewFrame"]')!);

    expect(screen.getByText('Open with Default App')).toBeInTheDocument();
    expect(screen.getByText(/Reveal in Finder|Show in Explorer/)).toBeInTheDocument();
    expect(screen.getByText('Open in New Window')).toBeInTheDocument();
    view.unmount();
  });

  it('shows the shared broken-thumbnail artwork in item and scope previews', () => {
    const entityView = renderInspector({ target: { kind: 'item', itemId: 1 }, data: entity });
    fireEvent.error(document.querySelector('img[src*="file-1"]')!);
    expect(document.querySelector('[data-broken-thumbnail]')).toBeInTheDocument();
    entityView.unmount();

    const scope = {
      node: { id: 'system:active', kind: 'system', name: 'All', count: 1, meta: {}, icon: null, color: null },
      totalCount: 1,
      totalSizeBytes: null,
      searchText: '',
      previewItems: [{ display_file_hash: 'file-1' }],
      description: 'All items.',
      folder: null,
      smartFolder: null,
    };
    const scopeView = renderInspector({ target: { kind: 'scope', nodeId: scope.node.id }, scope });
    fireEvent.error(document.querySelector('img[src*="file-1"]')!);
    expect(document.querySelector('[data-broken-thumbnail]')).toBeInTheDocument();
    scopeView.unmount();
  });

  it('shows a font specimen directly instead of treating a font as broken', () => {
    const fontEntity = {
      ...entity,
      media: [{ ...entity.media[0], file_hash: 'font-1', mime_type: 'font/ttf' }],
    };
    renderInspector({ target: { kind: 'item', itemId: 1 }, data: fontEntity });

    expect(document.querySelector('img[src*="font-1"]')).not.toBeInTheDocument();
    expect(document.querySelector('[data-font-thumbnail]')).toBeInTheDocument();
    expect(document.querySelector('[data-broken-thumbnail]')).not.toBeInTheDocument();
  });

  it('waits 250ms before showing aggregate loaders and swaps the summary atomically', async () => {
    vi.useFakeTimers();
    let resolveSummary: ((value: never) => void) | undefined;
    const summaryPromise = new Promise((resolve) => { resolveSummary = resolve; });
    vi.mocked(entityMutations.getTargetSelectionSummary).mockReturnValue(summaryPromise as never);
    const selectionTarget = { kind: 'query', query: { scope: { kind: 'all' }, filters: {}, sort: {} }, excluded_item_ids: [] };
    const view = renderInspector({
      target: { kind: 'multi', count: 2, selectionMode: 'query_results' },
      selectionTarget,
      selectionCount: 2,
    });

    expect(document.querySelectorAll('[data-inspector-summary-loading]')).toHaveLength(0);
    expect(document.querySelector('[data-inspector-action="add-tags"]')).not.toBeInTheDocument();

    act(() => { vi.advanceTimersByTime(249); });
    expect(document.querySelectorAll('[data-inspector-summary-loading]')).toHaveLength(0);
    act(() => { vi.advanceTimersByTime(1); });
    expect(document.querySelectorAll('[data-inspector-summary-loading]').length).toBeGreaterThan(0);

    await act(async () => {
      resolveSummary?.({
        total_count: 2,
        selected_count: 2,
        sample_hashes: ['file-1', 'file-2'],
        shared_tags: [{ tag: 'general:shared', count: 2 }],
        shared_folders: [{ folder_id: 7, name: 'Shared folder' }],
        shared_notes: 'Shared note',
        has_notes: true,
        shared_source_urls: ['https://example.com/shared'],
        has_source_urls: true,
        stats: {
          total_size_bytes: 300,
          media_count: 2,
          all_media_are_images: true,
          rating_stats: { min: 3, max: 3, shared: 3 },
        },
        revision: 4,
      } as never);
      await summaryPromise;
    });

    expect(document.querySelectorAll('[data-inspector-summary-loading]')).toHaveLength(0);
    expect(screen.getByText('shared')).toBeInTheDocument();
    expect(screen.getByText('Shared folder')).toBeInTheDocument();
    expect(screen.getByText('Shared note')).toBeInTheDocument();
    expect(screen.getByText('example.com')).toBeInTheDocument();
    expect(document.querySelector('[data-inspector-core-property="Size"]')).toHaveTextContent('300 B');
    view.unmount();
    vi.useRealTimers();
  });

  it('stacks six recent previews from 30% to full opacity with the newest on top', async () => {
    vi.mocked(entityMutations.getTargetSelectionSummary).mockResolvedValue({
      total_count: 6,
      selected_count: 6,
      sample_hashes: ['file-1', 'file-2', 'file-3', 'file-4', 'file-5', 'file-6'],
      shared_tags: [],
      shared_folders: [],
      shared_notes: null,
      has_notes: false,
      shared_source_urls: [],
      has_source_urls: false,
      stats: {
        total_size_bytes: 600,
        media_count: 6,
        all_media_are_images: true,
        rating_stats: { min: null, max: null, shared: null },
      },
      revision: 4,
    } as never);

    const view = renderInspector({
      target: { kind: 'multi', count: 6, selectionMode: 'explicit' },
      selectionTarget: { kind: 'explicit', item_ids: [1, 2, 3, 4, 5, 6] },
      selectionCount: 6,
    });

    await screen.findByText('6 items selected');
    const previews = [...document.querySelectorAll('[data-inspector-preview-hash]')];
    expect(previews.map((node) => node.getAttribute('data-inspector-preview-hash')))
      .toEqual(['file-1', 'file-2', 'file-3', 'file-4', 'file-5', 'file-6']);
    expect(previews.map((node) => node.getAttribute('data-inspector-stack-position')))
      .toEqual(['behind', 'behind', 'behind', 'behind', 'behind', 'top']);
    expect((previews[0] as HTMLElement).style.opacity).toBe('0.3');
    expect((previews[5] as HTMLElement).style.opacity).toBe('1');
    view.unmount();
  });

  it('confirms before replacing existing notes or sources across a selection', async () => {
    const summary = {
      total_count: 2,
      selected_count: 2,
      sample_hashes: ['file-1', 'file-2'],
      shared_tags: [],
      shared_folders: [],
      shared_notes: 'Existing note',
      has_notes: true,
      shared_source_urls: ['https://example.com/old'],
      has_source_urls: true,
      stats: {
        total_size_bytes: 300,
        media_count: 2,
        all_media_are_images: true,
        rating_stats: { min: null, max: null, shared: null },
      },
      revision: 4,
    };
    vi.mocked(entityMutations.getTargetSelectionSummary).mockResolvedValue(summary as never);
    const selectionTarget = { kind: 'explicit', item_ids: [1, 2] };
    const view = renderInspector({
      target: { kind: 'multi', count: 2, selectionMode: 'explicit' },
      selectionTarget,
      selectionCount: 2,
    });

    await screen.findByText('Existing note');
    const notes = document.querySelector('[data-inspector-anchor="notes"]')!;
    fireEvent.click(within(notes as HTMLElement).getByRole('button'));
    const textarea = notes.querySelector('textarea')!;
    fireEvent.change(textarea, { target: { value: 'Replacement note' } });
    fireEvent.keyDown(textarea, { key: 'Enter' });

    let confirm = getDefaultStore().get(confirmModalAtom);
    expect(confirm).toMatchObject({
      open: true,
      title: 'Overwrite notes?',
      confirmLabel: 'Overwrite Notes',
    });
    expect(confirm.message).toContain('all 2 selected items');
    expect(entityMutations.setTargetNotes).not.toHaveBeenCalled();
    confirm.onConfirm();
    expect(entityMutations.setTargetNotes).toHaveBeenCalledWith(selectionTarget, 'Replacement note');

    getDefaultStore().set(confirmModalAtom, { open: false, title: '', message: '', onConfirm: () => {} });
    const source = document.querySelector('[data-inspector-anchor="source"]')!;
    fireEvent.click(within(source as HTMLElement).getByRole('button'));
    fireEvent.click(within(source as HTMLElement).getByRole('button', { name: 'Remove' }));

    confirm = getDefaultStore().get(confirmModalAtom);
    expect(confirm).toMatchObject({
      open: true,
      title: 'Overwrite sources?',
      confirmLabel: 'Overwrite Sources',
    });
    expect(entityMutations.setTargetSourceUrls).not.toHaveBeenCalled();
    confirm.onConfirm();
    expect(entityMutations.setTargetSourceUrls).toHaveBeenCalledWith(selectionTarget, []);
    view.unmount();
  });

  it('renders a group from ordered replacement media details', () => {
    renderInspector({ target: { kind: 'item', itemId: 10 }, data: group });
    expect(screen.getByText('Album')).toBeInTheDocument();
    expect(document.querySelector('[data-inspector-format-label]')).toHaveAttribute('aria-label', 'Group');
    expect(document.querySelector('[data-inspector-format-label] [data-picto-icon="group"]')).toBeInTheDocument();
    expect(document.querySelector('[data-inspector-format-label]')).not.toHaveTextContent('JPG');
    expect(document.querySelector('[data-inspector-core-property="Items"]')).toHaveTextContent('2');
    expect(document.querySelector('[data-inspector-core-property="Type"]')).toHaveTextContent('Mixed');
    expect(screen.getByText('member-tag')).toBeInTheDocument();
  });

  it('renders full empty Tags/Folders controls and opens the existing selector portals', () => {
    const view = renderInspector({ target: { kind: 'item', itemId: 1 }, data: entity });
    const store = getDefaultStore();
    const addTags = document.querySelector('[data-inspector-empty-action="add-tags"]');
    const addFolder = document.querySelector('[data-inspector-empty-action="add-folder"]');

    expect(addTags).toHaveTextContent('Add Tags');
    expect(addFolder).toHaveTextContent('Add to Folder');
    expect(addTags).toHaveAttribute('data-inspector-button-variant', 'empty-section');
    expect(addFolder).toHaveAttribute('data-inspector-button-variant', 'empty-section');

    fireEvent.click(addTags!);
    expect(store.get(tagSelectPortalAtom)).toMatchObject({ open: true });
    fireEvent.click(addFolder!);
    expect(store.get(folderPickerPortalAtom)).toMatchObject({ open: true });

    store.set(folderPickerPortalAtom, { open: false, anchor: null });
    fireEvent.contextMenu(screen.getByText('Folders'));
    expect(store.get(folderPickerPortalAtom)).toMatchObject({ open: true });

    fireEvent.click(screen.getByText('Tags'));
    expect(document.querySelector('[data-inspector-empty-action="add-tags"]')).not.toBeInTheDocument();
    fireEvent.click(screen.getByText('Tags'));
    expect(document.querySelector('[data-inspector-empty-action="add-tags"]')).toBeInTheDocument();
    view.unmount();
  });

  it('uses compact icon-only add controls when Tags and Folders are populated', () => {
    const populated = {
      ...entity,
      aggregate_tags: ['creator:Example'],
      folder_ids: [1],
    };
    const view = renderInspector({ target: { kind: 'item', itemId: 1 }, data: populated });

    const addTags = screen.getByRole('button', { name: 'Add Tags' });
    const addFolder = screen.getByRole('button', { name: 'Add to Folder' });
    expect(addTags).not.toHaveTextContent('Add Tags');
    expect(addFolder).not.toHaveTextContent('Add to Folder');
    expect(addTags.querySelector('svg')).toHaveAttribute('width', '15');
    expect(addFolder.querySelector('svg')).toHaveAttribute('width', '15');
    expect(document.querySelector('[data-inspector-empty-action]')).not.toBeInTheDocument();
    view.unmount();
  });

  it('keeps Auto Tag in inspector scroll flow with its local action variant', () => {
    const entityView = renderInspector({ target: { kind: 'item', itemId: 1 }, data: entity });
    const action = document.querySelector('[data-inspector-action="auto-tag"]');
    expect(action).toHaveAttribute('data-inspector-button-variant', 'flow');
    expect(action?.closest('[data-inspector-scroll-content]')).toBeInTheDocument();
    expect(action?.closest('[data-inspector-section="actions"]')).toBeInTheDocument();
    expect(action?.querySelector('svg')).toHaveAttribute('width', '14');
    entityView.unmount();

    const multiView = renderInspector({ target: { kind: 'multi', count: 2, selectionMode: 'explicit' } });
    expect(document.querySelector('[data-inspector-action="auto-tag"]')).toBeDisabled();
    multiView.unmount();
  });

  it('places Export inside Properties and opens the shared export workflow', () => {
    const view = renderInspector({ target: { kind: 'item', itemId: 1 }, data: entity });
    const exportButton = screen.getByRole('button', { name: 'Export' });

    expect(exportButton).toHaveAttribute('data-inspector-button-variant', 'flow');
    expect(exportButton.closest('[data-inspector-section="properties"]')).toBeInTheDocument();
    fireEvent.click(exportButton);
    expect(getDefaultStore().get(exportModalAtom)).toEqual({
      open: true,
      fileCount: 1,
      target: { kind: 'explicit', item_ids: [1] },
    });
    view.unmount();
  });

  it('exports non-empty folder scopes but not system or empty scopes', () => {
    const folderScope = {
      node: { id: 'folder:7', kind: 'folder', name: 'Folder', count: 3, meta: {}, icon: null, color: null },
      totalCount: 3, totalSizeBytes: null, searchText: '', previewItems: [], description: null,
      folder: { folderId: 7, notes: null, autoTags: [], watchEnabled: false },
      smartFolder: null,
    };
    const folderView = renderInspector({ target: { kind: 'scope', nodeId: 'folder:7' }, scope: folderScope });
    expect(screen.getByRole('button', { name: 'Export' })).toBeInTheDocument();
    folderView.unmount();

    const systemView = renderInspector({
      target: { kind: 'scope', nodeId: 'system:active' },
      scope: { ...folderScope, node: { ...folderScope.node, id: 'system:active', kind: 'system' }, folder: null },
    });
    expect(screen.queryByRole('button', { name: 'Export' })).not.toBeInTheDocument();
    systemView.unmount();

    const emptyView = renderInspector({
      target: { kind: 'scope', nodeId: 'folder:7' },
      scope: { ...folderScope, totalCount: 0 },
    });
    expect(screen.queryByRole('button', { name: 'Export' })).not.toBeInTheDocument();
    emptyView.unmount();
  });

  it('keeps identity and core anchors for folder, smart-folder, and system scopes', () => {
    for (const kind of ['folder', 'smart_folder', 'system'] as const) {
      const scope = {
        node: { id: `${kind}:1`, kind, name: 'Example', count: 1, meta: {}, icon: null, color: null },
        totalCount: 1, totalSizeBytes: null, searchText: '', previewItems: [], description: null,
        folder: kind === 'folder' ? { folderId: 1, notes: null, autoTags: [], watchEnabled: false } : null,
        smartFolder: kind === 'smart_folder' ? { smartFolderId: 1, parentId: null, notes: null, predicate: null } : null,
      };
      const view = renderInspector({ target: { kind: 'scope', nodeId: scope.node.id }, scope });
      expect([...document.querySelectorAll('[data-inspector-anchor]')].map((node) => node.getAttribute('data-inspector-anchor')))
        .toEqual(kind === 'system' ? ['name'] : ['name', 'notes']);
      expect([...document.querySelectorAll('[data-inspector-core-property]')].map((node) => node.getAttribute('data-inspector-core-property')))
        .toEqual(['Items']);
      expect(screen.queryByText('Rating')).not.toBeInTheDocument();
      assertNoClassificationSections();
      view.unmount();
    }
  });
});
