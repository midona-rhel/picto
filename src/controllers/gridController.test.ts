import { getDefaultStore } from 'jotai';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { CanonicalEntityGridItem, EntityViewPage, EntityViewQuery } from '../shared/types/canonical';
import {
  currentGridQueryAtom,
  gridCursorAtom,
  gridErrorAtom,
  gridItemsAtom,
  gridLoadingAtom,
  gridShowSubfoldersAtom,
  gridSpacingAtom,
  gridSessionAtom,
  gridTotalCountAtom,
  gridTotalSizeBytesAtom,
} from '../state/grid';
import {
  emptyGridSelection,
  gridSelectionAtom,
  reduceGridSelection,
  selectionTargetAtom,
  selectAllResultsAtom,
} from '../state/selection';

const { queryItemsMock, getViewPrefsMock, revisionState } = vi.hoisted(() => ({
  queryItemsMock: vi.fn(),
  getViewPrefsMock: vi.fn(),
  revisionState: { library: -Infinity },
}));

vi.mock('../platform/entityApi', () => ({ queryItems: queryItemsMock }));
vi.mock('../platform/settingsApi', () => ({
  GRID_DEFAULTS_SCOPE: 'grid:defaults',
  getViewPrefs: getViewPrefsMock,
  setViewPrefs: vi.fn().mockResolvedValue(undefined),
}));
vi.mock('../runtime/libraryInvalidation', () => ({
  libraryInvalidation: {
    latestRevision: vi.fn((resource: string) => resource === 'library' ? revisionState.library : -Infinity),
  },
}));

import { gridController } from './gridController';

const store = getDefaultStore();

function item(id: number): CanonicalEntityGridItem {
  return {
    root_id: id,
    kind: 'media',
    lifecycle: 'active',
    name: `Item ${id}`,
    cover_media_id: id,
    content_hash: `file-${id}`,
    mime: 'image/jpeg',
    width: 100,
    height: 100,
    duration_ms: null,
    frame_count: null,
    palette: [],
    imported_at_ms: id,
    captured_at_ms: null,
    modified_at_ms: id,
    media_count: 1,
    total_size_bytes: 100,
    rating: 'unrated',
  };
}

function page(
  items: CanonicalEntityGridItem[],
  visibleCount: number,
  nextCursor: string | null = items.length < visibleCount
    ? `cursor-after-${items[items.length - 1]?.root_id ?? 0}`
    : null,
): EntityViewPage {
  return {
    items,
    next_cursor: nextCursor,
    revision: 1,
    total: visibleCount,
    media_count: visibleCount,
    total_size_bytes: visibleCount * 100,
  };
}

function appendPage(items: CanonicalEntityGridItem[], nextCursor: string | null = null): EntityViewPage {
  return {
    items,
    next_cursor: nextCursor,
    revision: 1,
    total: items.length,
    media_count: items.length,
    total_size_bytes: items.length * 100,
  };
}

function queryText(query: EntityViewQuery): string | null {
  const expressions = query.view.filter.kind === 'all' ? query.view.filter.value : [query.view.filter];
  const text = expressions.find((expression) => (
    expression.kind === 'clause' && expression.value.clause === 'text'
  ));
  return text?.kind === 'clause' && text.value.clause === 'text' ? text.value.query : null;
}

describe('gridController pagination', () => {
  beforeEach(() => {
    queryItemsMock.mockReset();
    getViewPrefsMock.mockReset();
    getViewPrefsMock.mockResolvedValue({});
    revisionState.library = -Infinity;
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      scope: { kind: 'all' },
      sort: { field: 'imported_at', direction: 'descending' },
      items: [item(1)],
      cursor: 'cursor-after-1',
      totalCount: 2,
      totalSizeBytes: 200,
      revision: 1,
      status: 'idle',
      error: null,
      generation: 0,
    });
    store.set(gridSelectionAtom, emptyGridSelection());
  });

  it('loads global defaults and the sparse scope override when navigating', async () => {
    queryItemsMock.mockResolvedValueOnce(page([item(1)], 1));

    await gridController.navigateTo({ kind: 'all' });

    expect(getViewPrefsMock.mock.calls.map(([scope]) => scope)).toEqual([
      'grid:defaults',
      'system:active',
    ]);
  });

  it('prepares the complete replacement without changing the visible session', async () => {
    getViewPrefsMock.mockImplementation(async (scope: string) => scope === 'grid:defaults'
      ? { view_mode: 'grid', target_size: 333 }
      : { show_subfolders: false });
    let resolvePage: ((value: EntityViewPage) => void) | undefined;
    queryItemsMock.mockImplementationOnce(() => new Promise<EntityViewPage>((resolve) => {
      resolvePage = resolve;
    }));
    store.set(gridSelectionAtom, {
      mode: 'explicit',
      itemIds: new Set([1]),
      excludedItemIds: new Set<number>(),
      folderNodeIds: new Set<string>(),
      anchor: { kind: 'item', id: 1 },
    });

    const navigation = gridController.prepareNavigation({ kind: 'folder', folder_id: 7 });
    await vi.waitFor(() => expect(queryItemsMock).toHaveBeenCalledOnce());

    expect(store.get(gridLoadingAtom)).toBe(false);
    expect(store.get(gridSessionAtom).scope).toEqual({ kind: 'all' });
    expect(store.get(gridItemsAtom).map((entry) => entry.root_id)).toEqual([1]);
    expect(store.get(gridTotalCountAtom)).toBe(2);
    expect(store.get(gridSessionAtom).view).toEqual(expect.objectContaining({ mode: 'waterfall' }));
    expect(store.get(gridSelectionAtom).itemIds).toEqual(new Set([1]));

    resolvePage?.(page([item(7)], 1));
    const prepared = await navigation;
    expect(store.get(gridSessionAtom).scope).toEqual({ kind: 'all' });
    expect(store.get(gridItemsAtom).map((entry) => entry.root_id)).toEqual([1]);
    expect(store.get(gridSessionAtom).view.mode).toBe('waterfall');
    expect(prepared.session.view).toEqual(expect.objectContaining({
      mode: 'grid',
      targetSize: 333,
      showSubfolders: false,
    }));

    gridController.commitNavigation(prepared);
    expect(store.get(gridSessionAtom).scope).toEqual({ kind: 'folder', folder_id: 7 });
    expect(store.get(gridItemsAtom).map((entry) => entry.root_id)).toEqual([7]);
    expect(store.get(gridLoadingAtom)).toBe(false);
    expect(store.get(gridSessionAtom).view).toEqual(expect.objectContaining({
      mode: 'grid',
      targetSize: 333,
      showSubfolders: false,
    }));
    expect(store.get(gridSelectionAtom).itemIds).toEqual(new Set());
  });

  it('lets untouched folders inherit the global sort defaults', async () => {
    getViewPrefsMock.mockImplementation(async (scope: string) => scope === 'grid:defaults'
      ? { sort_field: 'name', sort_order: 'ascending' }
      : {});
    queryItemsMock.mockResolvedValueOnce(page([item(1)], 1));

    await gridController.navigateTo({ kind: 'folder', folder_id: 7 });

    expect(queryItemsMock.mock.calls[0][0].view.sort).toEqual({
      field: 'name',
      direction: 'ascending',
      random_seed: null,
    });
  });

  it('uses an explicit stable seed for a Random visit', async () => {
    queryItemsMock.mockResolvedValueOnce(page([item(1)], 1));

    await gridController.navigateTo({ kind: 'all' }, {
      sort: { field: 'random', direction: 'ascending', random_seed: 'visit-1' },
    });

    expect(queryItemsMock.mock.calls[0][0].view.sort).toEqual({
      field: 'random',
      direction: 'ascending',
      random_seed: 'visit-1',
    });
  });

  it('restores the saved subfolder visibility for folder views', async () => {
    getViewPrefsMock.mockImplementation(async (scope: string) => scope === 'grid:defaults'
      ? { show_subfolders: true }
      : { show_subfolders: false });
    queryItemsMock.mockResolvedValueOnce(page([item(1)], 1));

    await gridController.navigateTo({ kind: 'folder', folder_id: 7 });

    expect(store.get(gridShowSubfoldersAtom)).toBe(false);
    expect(queryItemsMock.mock.calls[0][0].scope).toEqual({ kind: 'folder', folder_id: 7 });
  });

  it('queries the complete folder tree when subfolder content is enabled', async () => {
    getViewPrefsMock.mockImplementation(async (scope: string) => scope === 'grid:defaults'
      ? { show_subfolders: true }
      : {});
    queryItemsMock.mockResolvedValueOnce(page([item(1)], 1));

    await gridController.navigateTo({ kind: 'folder', folder_id: 7 });

    expect(store.get(gridShowSubfoldersAtom)).toBe(true);
    expect(queryItemsMock.mock.calls[0][0].scope).toEqual({ kind: 'folder_tree', folder_id: 7 });
  });

  it('does not expose recursive content semantics to smart folders', async () => {
    getViewPrefsMock.mockImplementation(async () => ({ show_subfolders: true }));
    queryItemsMock.mockResolvedValueOnce(page([item(1)], 1));

    await gridController.navigateTo({ kind: 'smart_folder', smart_folder_id: 9 });

    expect(store.get(gridShowSubfoldersAtom)).toBe(false);
    expect(queryItemsMock.mock.calls[0][0].scope).toEqual({ kind: 'smart_folder', smart_folder_id: 9 });
  });

  it('requeries the active folder when recursive content is toggled', async () => {
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      scope: { kind: 'folder', folder_id: 7 },
      view: { ...store.get(gridSessionAtom).view, showSubfolders: false },
      status: 'idle',
    });
    queryItemsMock.mockResolvedValueOnce(page([item(2)], 1));

    gridController.applyIntent({ type: 'view', patch: { showSubfolders: true } });
    await vi.waitFor(() => expect(store.get(gridLoadingAtom)).toBe(false));

    expect(queryItemsMock).toHaveBeenCalledOnce();
    expect(queryItemsMock.mock.calls[0][0].scope).toEqual({ kind: 'folder_tree', folder_id: 7 });
  });

  it('lets a scope override the application grid spacing without requerying', async () => {
    getViewPrefsMock.mockImplementation(async (scope: string) => scope === 'grid:defaults'
      ? { spacing: 'wide' }
      : { spacing: 'tight' });
    queryItemsMock.mockResolvedValueOnce(page([item(1)], 1));

    await gridController.navigateTo({ kind: 'folder', folder_id: 7 });

    expect(store.get(gridSpacingAtom)).toBe('tight');
    const callsBeforeChange = queryItemsMock.mock.calls.length;
    gridController.updateView({ spacing: 'wide' });
    gridController.saveViewPref({ spacing: 'wide' });
    expect(store.get(gridSpacingAtom)).toBe('wide');
    expect(queryItemsMock).toHaveBeenCalledTimes(callsBeforeChange);
  });

  it('defaults Inbox to oldest first without inheriting the application sort', async () => {
    getViewPrefsMock.mockImplementation(async (scope: string) => scope === 'grid:defaults'
      ? { sort_field: 'name', sort_order: 'descending' }
      : {});
    queryItemsMock.mockResolvedValueOnce(page([item(1)], 1));

    await gridController.navigateTo({ kind: 'inbox' });

    expect(queryItemsMock.mock.calls[0][0].view.sort).toEqual({
      field: 'imported_at',
      direction: 'ascending',
      random_seed: null,
    });

  });

  it('uses and changes the Inbox-specific sort like any other view', async () => {
    getViewPrefsMock.mockImplementation(async (scope: string) => scope === 'grid:defaults'
      ? { sort_field: 'name', sort_order: 'ascending' }
      : { sort_field: 'rating', sort_order: 'descending' });
    queryItemsMock.mockResolvedValue(page([item(1)], 1));

    await gridController.navigateTo({ kind: 'inbox' });

    expect(queryItemsMock.mock.calls[0][0].view.sort).toEqual({
      field: 'rating',
      direction: 'descending',
      random_seed: null,
    });

    gridController.applyIntent({ type: 'sort', field: 'name', direction: 'ascending' });
    await vi.waitFor(() => expect(queryItemsMock).toHaveBeenCalledTimes(2));

    expect(queryItemsMock.mock.calls[1][0].view.sort).toEqual({
      field: 'name',
      direction: 'ascending',
      random_seed: null,
    });
  });

  it('queries the replacement page contract and appends by item_id', async () => {
    queryItemsMock.mockResolvedValueOnce(appendPage([item(2)]));

    await gridController.loadNextPage();

    expect(queryItemsMock).toHaveBeenCalledTimes(1);
    expect(queryItemsMock.mock.calls[0][1]).toEqual({ cursor: 'cursor-after-1', limit: 500 });
    expect(store.get(gridItemsAtom).map((entry) => entry.root_id)).toEqual([1, 2]);
    expect(store.get(gridCursorAtom)).toBeNull();
    expect(store.get(gridTotalCountAtom)).toBe(2);
    expect(store.get(gridTotalSizeBytesAtom)).toBe(200);
    expect(store.get(gridLoadingAtom)).toBe(false);
  });

  it('uses one guarded append path for sequential pages', async () => {
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), totalCount: 3 });
    queryItemsMock
      .mockResolvedValueOnce(appendPage([item(2)], 'cursor-after-2'))
      .mockResolvedValueOnce(appendPage([item(3)]));

    await gridController.loadNextPage();
    await gridController.loadNextPage();

    expect(queryItemsMock).toHaveBeenCalledTimes(2);
    expect(queryItemsMock.mock.calls.map((call) => call[1].cursor)).toEqual([
      'cursor-after-1',
      'cursor-after-2',
    ]);
    expect(store.get(gridItemsAtom).map((entry) => entry.root_id)).toEqual([1, 2, 3]);
    expect(store.get(gridErrorAtom)).toBeNull();
    expect(store.get(gridLoadingAtom)).toBe(false);
  });

  it('deduplicates concurrent requests for the same cursor', async () => {
    let resolvePage: ((value: EntityViewPage) => void) | undefined;
    queryItemsMock.mockImplementationOnce(() => new Promise<EntityViewPage>((resolve) => { resolvePage = resolve; }));

    const first = gridController.loadNextPage();
    const second = gridController.loadNextPage();
    resolvePage?.(page([item(2)], 2));
    await Promise.all([first, second]);

    expect(queryItemsMock).toHaveBeenCalledTimes(1);
    expect(store.get(gridItemsAtom).map((entry) => entry.root_id)).toEqual([1, 2]);
  });

  it('keeps the cursor after an append failure so pagination can retry', async () => {
    queryItemsMock.mockRejectedValueOnce(new Error('page failed'));

    await gridController.loadNextPage();

    expect(store.get(gridCursorAtom)).toBe('cursor-after-1');
    expect(store.get(gridErrorAtom)).toBe('page failed');
    expect(store.get(gridLoadingAtom)).toBe(false);

    queryItemsMock.mockResolvedValueOnce(page([item(2)], 2));
    await gridController.loadNextPage();
    expect(queryItemsMock).toHaveBeenCalledTimes(2);
  });

  it('builds a complete replacement ItemQuery without hash or page fields', async () => {
    queryItemsMock.mockResolvedValueOnce(page([item(1)], 1));

    await gridController.loadFirstPage();

    const query = queryItemsMock.mock.calls[0][0];
    expect(query).toEqual(store.get(currentGridQueryAtom));
    expect(query).toEqual({
      scope: { kind: 'all' },
      view: {
        filter: { kind: 'all', value: [] },
        sort: { field: 'imported_at', direction: 'descending', random_seed: null },
      },
    });
    expect('entity_hash' in query).toBe(false);
    expect('page' in query).toBe(false);
  });

  it('keeps Command+A as its snapshotted query plus exclusions', () => {
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      searchText: '',
      filters: { ...store.get(gridSessionAtom).filters, text: 'before' },
      totalCount: 100_000,
    });
    store.set(selectAllResultsAtom);
    const target = store.get(selectionTargetAtom);

    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      filters: { ...store.get(gridSessionAtom).filters, text: 'after' },
    });

    expect(target).toEqual({
      kind: 'query',
      query: expect.objectContaining({
        view: expect.objectContaining({
          filter: expect.objectContaining({
            value: expect.arrayContaining([
              { kind: 'clause', value: { clause: 'text', field: 'global', query: 'before' } },
            ]),
          }),
        }),
      }),
      excluded_root_ids: [],
    });
    expect(store.get(selectionTargetAtom)).toEqual(target);
  });

  it('represents loaded ranges with stable item IDs rather than page indexes', () => {
    const anchored = reduceGridSelection(emptyGridSelection(), {
      type: 'replace_items',
      itemIds: new Set([90]),
      anchor: 90,
    });
    const range = reduceGridSelection(anchored, {
      type: 'range_items',
      itemIds: new Set([90, 12, 405]),
    });

    expect(range.anchor).toEqual({ kind: 'item', id: 90 });
    expect(range.itemIds).toEqual(new Set([90, 12, 405]));
  });

  it('ignores an identical filter intent without querying or changing generation', () => {
    const before = store.get(gridSessionAtom);

    gridController.applyIntent({ type: 'filter', filters: before.filters });

    expect(queryItemsMock).not.toHaveBeenCalled();
    expect(store.get(gridSessionAtom)).toBe(before);
  });

  it('keeps the rendered page identity when a changed filter returns the same rows', async () => {
    const before = store.get(gridItemsAtom);
    queryItemsMock.mockResolvedValueOnce(page(before.map((entry) => ({ ...entry })), 1));

    gridController.applyIntent({
      type: 'filter',
      filters: { ...store.get(gridSessionAtom).filters, color_hex: '#00FF00' },
    });
    await vi.waitFor(() => expect(store.get(gridLoadingAtom)).toBe(false));

    expect(store.get(gridItemsAtom)).toBe(before);
  });

  it('reconciles the loaded window without loading state or identity churn', async () => {
    const first = item(1);
    const second = item(2);
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      items: [first, second],
      cursor: null,
      totalCount: 2,
      generation: 4,
      status: 'idle',
    });
    let resolvePage: ((value: EntityViewPage) => void) | undefined;
    queryItemsMock.mockImplementationOnce(() => new Promise<EntityViewPage>((resolve) => {
      resolvePage = resolve;
    }));

    const reconcile = gridController.reconcile();
    expect(store.get(gridSessionAtom).status).toBe('idle');
    expect(store.get(gridSessionAtom).generation).toBe(4);

    resolvePage?.(page([{ ...first }, { ...second }, item(3)], 3));
    await expect(reconcile).resolves.toBe(true);
    const after = store.get(gridSessionAtom);
    expect(queryItemsMock.mock.calls[0][1]).toEqual({ cursor: null, limit: 500 });
    expect(after.items).toHaveLength(3);
    expect(after.items[0]).toBe(first);
    expect(after.items[1]).toBe(second);
    expect(after.generation).toBe(4);
    expect(after.status).toBe('idle');
  });

  it('preserves every already-loaded page while refreshing only the bounded first page', async () => {
    const loaded = Array.from({ length: 750 }, (_, index) => item(index + 1));
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      items: loaded,
      cursor: 'cursor-after-750',
      totalCount: 900,
    });
    queryItemsMock
      .mockResolvedValueOnce(page(
        loaded.slice(0, 500).map((entry) => ({ ...entry })),
        900,
        'cursor-after-500',
      ))
      .mockResolvedValueOnce(appendPage(
        loaded.slice(500).map((entry) => ({ ...entry })),
        'cursor-after-750',
      ));

    await gridController.reconcile();

    expect(queryItemsMock.mock.calls.map((call) => call[1])).toEqual([
      { cursor: null, limit: 500 },
      { cursor: 'cursor-after-500', limit: 250 },
    ]);
    expect(store.get(gridSessionAtom).items).toHaveLength(750);
    expect(store.get(gridSessionAtom).items[749]).toBe(loaded[749]);
    expect(store.get(gridCursorAtom)).toBe('cursor-after-750');
  });

  it('removes affected stale rows and fills a complete loaded window from the tail', async () => {
    const loaded = Array.from({ length: 501 }, (_, index) => item(index + 1));
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      items: loaded,
      cursor: null,
      totalCount: 501,
    });
    queryItemsMock
      .mockResolvedValueOnce(page(loaded.slice(1, 501), 501, 'cursor-after-501'))
      .mockResolvedValueOnce(appendPage([item(502)]));

    await gridController.reconcile([1]);

    expect(queryItemsMock.mock.calls.map((call) => call[1])).toEqual([
      { cursor: null, limit: 500 },
      { cursor: 'cursor-after-501', limit: 1 },
    ]);
    expect(store.get(gridSessionAtom).items.map((entry) => entry.root_id)).not.toContain(1);
    const reconciled = store.get(gridSessionAtom).items;
    expect(reconciled[reconciled.length - 1]?.root_id).toBe(502);
  });

  it('settles search after 250 ms of inactivity', async () => {
    vi.useFakeTimers();
    queryItemsMock.mockResolvedValueOnce(page([item(1)], 1));

    gridController.setSearchText('alice');
    await vi.advanceTimersByTimeAsync(249);
    expect(queryItemsMock).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(1);
    expect(queryItemsMock).toHaveBeenCalledOnce();
    expect(queryText(queryItemsMock.mock.calls[0][0])).toBe('alice');
    vi.useRealTimers();
  });

  it('coalesces settled text while a native search is still running', async () => {
    vi.useFakeTimers();
    let resolveFirst: ((value: EntityViewPage) => void) | undefined;
    queryItemsMock
      .mockImplementationOnce(() => new Promise<EntityViewPage>((resolve) => {
        resolveFirst = resolve;
      }))
      .mockResolvedValueOnce(page([item(2)], 1));

    gridController.setSearchText('alice');
    await vi.advanceTimersByTimeAsync(250);
    expect(queryItemsMock).toHaveBeenCalledOnce();

    gridController.setSearchText('bob');
    await vi.advanceTimersByTimeAsync(250);
    gridController.setSearchText('carol');
    await vi.advanceTimersByTimeAsync(250);
    expect(queryItemsMock).toHaveBeenCalledOnce();

    resolveFirst?.(page([item(1)], 1));
    await vi.waitFor(() => expect(queryItemsMock).toHaveBeenCalledTimes(2));
    expect(queryText(queryItemsMock.mock.calls[1][0])).toBe('carol');
    vi.useRealTimers();
  });

  it('does not commit a superseded search response', async () => {
    vi.useFakeTimers();
    let resolveFirst: ((value: EntityViewPage) => void) | undefined;
    queryItemsMock
      .mockImplementationOnce(() => new Promise<EntityViewPage>((resolve) => { resolveFirst = resolve; }))
      .mockResolvedValueOnce(page([item(3)], 1));

    gridController.setSearchText('old');
    await vi.advanceTimersByTimeAsync(250);
    gridController.setSearchText('new');
    await vi.advanceTimersByTimeAsync(250);
    resolveFirst?.(page([item(2)], 1));

    await vi.waitFor(() => expect(queryItemsMock).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(store.get(gridItemsAtom).map((entry) => entry.root_id)).toEqual([3]));
    expect(queryText(queryItemsMock.mock.calls[1][0])).toBe('new');
    vi.useRealTimers();
  });

  it('retries a first page whose revision predates the latest library invalidation', async () => {
    revisionState.library = 5;
    queryItemsMock
      .mockResolvedValueOnce({ ...page([item(1)], 1), revision: 4 })
      .mockResolvedValueOnce({ ...page([item(2)], 1), revision: 5 });

    await gridController.loadFirstPage();

    expect(queryItemsMock).toHaveBeenCalledTimes(2);
    expect(store.get(gridItemsAtom).map((entry) => entry.root_id)).toEqual([2]);
    expect(store.get(gridSessionAtom).revision).toBe(5);
  });
});
