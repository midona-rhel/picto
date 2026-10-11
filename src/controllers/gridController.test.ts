import { getDefaultStore } from 'jotai';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { CanonicalEntityGridItem, EntityViewPage, EntityViewQuery } from '../shared/types/canonical';
import {
  currentGridQueryAtom,
  gridErrorAtom,
  gridItemsAtom,
  gridQueryHeadItemsAtom,
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

const { queryItemsMock, supersedeWindowMock, getViewPrefsMock, revisionState } = vi.hoisted(() => ({
  queryItemsMock: vi.fn(),
  supersedeWindowMock: vi.fn().mockResolvedValue(undefined),
  getViewPrefsMock: vi.fn(),
  revisionState: { library: -Infinity },
}));

vi.mock('../platform/entityApi', () => ({ queryItemWindow: queryItemsMock, supersedeItemWindow: supersedeWindowMock }));
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
import { setViewPrefs } from '../platform/settingsApi';

it('turns a stalled preference read into a retryable destination error within ten seconds', async () => {
  vi.useFakeTimers();
  try {
    queryItemsMock.mockClear();
    getViewPrefsMock.mockReset().mockResolvedValue(null);
    getViewPrefsMock.mockImplementationOnce(() => new Promise(() => {}));
    const pending = gridController.prepareNavigation({ kind: 'all' });
    await vi.advanceTimersByTimeAsync(10_000);
    const result = await pending;
    expect(result.session.status).toBe('error');
    expect(result.session.error).toContain('Please retry');
    expect(queryItemsMock).not.toHaveBeenCalled();
  } finally { vi.useRealTimers(); }
});

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
): EntityViewPage & { start: number; head_items: CanonicalEntityGridItem[] } {
  return {
    start: 0,
    items,
    head_items: items.slice(0, 4),
    next_cursor: nextCursor,
    revision: 1,
    total: visibleCount,
    media_count: visibleCount,
    total_size_bytes: visibleCount * 100,
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
    gridController.deactivate();
    queryItemsMock.mockReset();
    getViewPrefsMock.mockReset();
    getViewPrefsMock.mockResolvedValue({});
    revisionState.library = -Infinity;
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      scope: { kind: 'all' },
      sort: { field: 'imported_at', direction: 'descending' },
      items: [item(1)],
      queryHeadItems: [item(1)],
      windowStart: 0,
      totalCount: 2,
      totalSizeBytes: 200,
      revision: 1,
      status: 'idle',
      active: true,
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

  it('replaces the loaded window with a bounded exact ordinal request', async () => {
    queryItemsMock.mockResolvedValueOnce(page([item(1), item(2)], 2));

    await gridController.requestWindowAt(1);

    expect(queryItemsMock).toHaveBeenCalledTimes(1);
    expect(queryItemsMock.mock.calls[0][1]).toEqual({ position: { kind: 'ordinal', value: 0 }, limit: 1500 });
    expect(store.get(gridItemsAtom).map((entry) => entry.root_id)).toEqual([1, 2]);
    expect(store.get(gridTotalCountAtom)).toBe(2);
    expect(store.get(gridTotalSizeBytesAtom)).toBe(200);
    expect(store.get(gridLoadingAtom)).toBe(false);
  });

  it('jumps directly to the last window and then back without retaining the prefix', async () => {
    const queryHead = [item(1), item(2), item(3), item(4)];
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      queryHeadItems: queryHead,
      totalCount: 1_000_000,
    });
    const tail = Array.from({ length: 1500 }, (_, i) => item(998501 + i));
    const head = Array.from({ length: 1500 }, (_, i) => item(1 + i));
    queryItemsMock
      .mockResolvedValueOnce({ ...page(tail, 1_000_000), start: 998500, head_items: queryHead })
      .mockResolvedValueOnce(page(head, 1_000_000));

    await gridController.requestWindowAt(999999);
    expect(store.get(gridItemsAtom)).toHaveLength(1500);
    expect(store.get(gridSessionAtom).windowStart).toBe(998500);
    expect(store.get(gridItemsAtom)[1499].root_id).toBe(1_000_000);
    expect(store.get(gridQueryHeadItemsAtom)).toBe(queryHead);
    await gridController.requestWindowAt(0);

    expect(queryItemsMock).toHaveBeenCalledTimes(2);
    expect(queryItemsMock.mock.calls.map((call) => call[1])).toEqual([
      { position: { kind: 'ordinal', value: 998500 }, limit: 1500 },
      { position: { kind: 'ordinal', value: 0 }, limit: 1500 },
    ]);
    expect(store.get(gridItemsAtom)).toHaveLength(1500);
    expect(store.get(gridItemsAtom)[0].root_id).toBe(1);
    expect(store.get(gridErrorAtom)).toBeNull();
    expect(store.get(gridLoadingAtom)).toBe(false);
  });

  it('deduplicates concurrent requests for the same window', async () => {
    let resolvePage: ((value: EntityViewPage) => void) | undefined;
    queryItemsMock.mockImplementationOnce(() => new Promise<EntityViewPage>((resolve) => { resolvePage = resolve; }));

    const first = gridController.requestWindowAt(1);
    const second = gridController.requestWindowAt(1);
    resolvePage?.(page([item(1), item(2)], 2));
    await Promise.all([first, second]);

    expect(queryItemsMock).toHaveBeenCalledTimes(1);
    expect(store.get(gridItemsAtom).map((entry) => entry.root_id)).toEqual([1, 2]);
  });

  it('prepares the million-item tail directly and keeps the actual query head for previews', async () => {
    queryItemsMock.mockResolvedValueOnce({ ...page([item(999_999)], 1_000_000), start: 998_500,
      head_items: [item(1), item(2), item(3), item(4)] });
    const before = store.get(gridSessionAtom);
    const prepared = await gridController.prepareNavigation({ kind: 'all' }, { restoreProgress: 1 });
    expect(queryItemsMock.mock.calls.map(call => call[1])).toEqual([
      { position: { kind: 'fraction', value: 1 }, limit: 1500 },
    ]);
    expect(store.get(gridSessionAtom)).toBe(before);
    gridController.commitNavigation(prepared);
    expect(store.get(gridSessionAtom).windowStart).toBe(998_500);
    expect(store.get(gridItemsAtom).map(entry => entry.root_id)).toEqual([999_999]);
    expect(store.get(gridQueryHeadItemsAtom).map(entry => entry.root_id)).toEqual([1, 2, 3, 4]);
  });

  it('prepares head and restored window in one snapshot without chasing ongoing imports', async () => {
    revisionState.library = 9;
    queryItemsMock.mockResolvedValueOnce({ ...page([item(8000)], 9000), revision: 2, start: 7500,
      head_items: [item(2)] });
    const prepared = await gridController.prepareNavigation({ kind: 'all' }, { restoreProgress: 1 });
    expect(queryItemsMock).toHaveBeenCalledOnce();
    expect(prepared.session.queryHeadItems[0].root_id).toBe(2);
    expect(prepared.session.windowStart).toBe(7500);
    expect(prepared.session.totalCount).toBe(9000);
  });

  it('asks for the saved spot directly when Back restores filters in the same scope', () => {
    queryItemsMock.mockResolvedValue(page([item(1)], 1));
    gridController.applyIntent({
      type: 'filter',
      filters: { ...store.get(gridSessionAtom).filters, text: 'back' },
      restoreScroll: true,
      restoreProgress: 1,
    });
    expect(queryItemsMock.mock.calls[queryItemsMock.mock.calls.length - 1][1]).toMatchObject({ position: { kind: 'fraction', value: 1 } });
    gridController.applyIntent({ type: 'filter', filters: { ...store.get(gridSessionAtom).filters, text: 'new' } });
    expect(queryItemsMock.mock.calls[queryItemsMock.mock.calls.length - 1][1]).toMatchObject({ position: { kind: 'ordinal', value: 0 } });
  });

  it('does not allow an obsolete preparation to commit after a newer destination', async () => {
    queryItemsMock.mockResolvedValueOnce(page([item(7)], 1));
    const old = await gridController.prepareNavigation({ kind: 'folder', folder_id: 7 });
    queryItemsMock.mockResolvedValueOnce(page([item(9)], 1));
    const latest = await gridController.prepareNavigation({ kind: 'folder', folder_id: 9 });
    gridController.commitNavigation(latest);
    gridController.commitNavigation(old);
    expect(store.get(gridItemsAtom)[0].root_id).toBe(9);
  });

  it('prepares sorting without changing the visible query and keeps the prepared random seed', async () => {
    const before = store.get(gridSessionAtom);
    queryItemsMock.mockResolvedValueOnce(page([item(8)], 1));
    const prepared = await gridController.prepareIntent({ type: 'sort', field: 'random', direction: 'ascending' });
    expect(store.get(gridSessionAtom)).toBe(before);
    const queriedSeed = queryItemsMock.mock.calls[0][0].view.sort.random_seed;
    expect(queriedSeed).toBeTruthy();
    gridController.commitNavigation(prepared);
    expect(store.get(gridSessionAtom).sort.randomSeed).toBe(queriedSeed);
    expect(store.get(gridItemsAtom)[0].root_id).toBe(8);
  });

  it('reuses resident metadata for a layout-only preparation', async () => {
    const before = store.get(gridSessionAtom);
    const prepared = await gridController.prepareIntent({ type: 'view', patch: { mode: 'justified' } });
    expect(queryItemsMock).not.toHaveBeenCalled();
    expect(store.get(gridSessionAtom)).toBe(before);
    gridController.commitNavigation(prepared);
    expect(store.get(gridSessionAtom).items).toBe(before.items);
    expect(store.get(gridSessionAtom).view.mode).toBe('justified');
  });

  it('displays a dragged folder order rather than falling back to recursive import order', async () => {
    const current = store.get(gridSessionAtom);
    store.set(gridSessionAtom, { ...current, scope: { kind: 'folder', folder_id: 7 },
      view: { ...current.view, showSubfolders: true }, items: [item(1), item(2)] });
    const selection = { ...emptyGridSelection(), itemIds: new Set([1]) };
    store.set(gridSelectionAtom, selection);
    queryItemsMock.mockResolvedValueOnce(page([item(2), item(1)], 2));

    const prepared = await gridController.prepareIntent({ type: 'sort', field: 'folder_order', direction: 'ascending' });
    expect(queryItemsMock.mock.calls[0][0]).toMatchObject({
      scope: { kind: 'folder', folder_id: 7 },
      view: { sort: { field: 'folder_order', direction: 'ascending' } },
    });
    expect(prepared.preferencePatch).toMatchObject({ show_subfolders: false, sort_field: 'folder_order' });
    gridController.commitNavigation(prepared);
    expect(store.get(gridItemsAtom).map(value => value.root_id)).toEqual([2, 1]);
    expect(store.get(gridSelectionAtom)).toBe(selection);
  });

  it('drops an obsolete result and requests only the latest queued destination', async () => {
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), totalCount: 1_000_000 });
    let finish: ((value: EntityViewPage & { start: number }) => void) | undefined;
    queryItemsMock.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }))
      .mockResolvedValueOnce({ ...page([item(100001)], 1_000_000), start: 100000 });
    const pending = gridController.requestWindowAt(999999);
    void gridController.requestWindowAt(500000);
    const latest = gridController.requestWindowAt(100500);
    finish?.({ ...page([item(999999)], 1_000_000), start: 998500 });
    await Promise.all([pending, latest]);
    expect(queryItemsMock.mock.calls.map(call => call[1].position.value)).toEqual([998500, 100000]);
    expect(store.get(gridItemsAtom).map(entry => entry.root_id)).toEqual([100001]);
  });

  it('keeps the previous window after a failure so the destination can retry', async () => {
    queryItemsMock.mockRejectedValueOnce(new Error('page failed'));

    await gridController.requestWindowAt(1);

    expect(store.get(gridSessionAtom).windowStart).toBe(0);
    expect(store.get(gridItemsAtom).map(entry => entry.root_id)).toEqual([1]);
    expect(store.get(gridErrorAtom)).toBe('page failed');
    expect(store.get(gridLoadingAtom)).toBe(false);

    queryItemsMock.mockResolvedValueOnce(page([item(2)], 2));
    await gridController.requestWindowAt(1);
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
      windowStart: 0,
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
    expect(queryItemsMock.mock.calls[0][1]).toEqual({ position: { kind: 'ordinal', value: 0 }, limit: 500 });
    expect(after.items).toHaveLength(3);
    expect(after.items[0]).toBe(first);
    expect(after.items[1]).toBe(second);
    expect(after.generation).toBe(4);
    expect(after.status).toBe('idle');
  });

  it('refreshes the current ordinal window and the canonical four-item query head', async () => {
    const loaded = Array.from({ length: 750 }, (_, index) => item(index + 1));
    const head = [item(9001), item(9002), item(9003), item(9004)];
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      items: loaded,
      windowStart: 500000,
      totalCount: 1_000_000,
    });
    queryItemsMock
      .mockResolvedValueOnce({ ...page(loaded.map(entry => ({ ...entry })), 1_000_000), start: 500000,
        head_items: head });

    await gridController.reconcile();

    expect(queryItemsMock.mock.calls.map((call) => call[1])).toEqual([
      { position: { kind: 'ordinal', value: 500000 }, limit: 750 },
    ]);
    expect(store.get(gridSessionAtom).items).toHaveLength(750);
    expect(store.get(gridSessionAtom).items[749]).toBe(loaded[749]);
    expect(store.get(gridSessionAtom).windowStart).toBe(500000);
    expect(store.get(gridQueryHeadItemsAtom).map((entry) => entry.root_id)).toEqual([9001, 9002, 9003, 9004]);
  });

  it('cancels obsolete backend work without showing its cancellation as an error', async () => {
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), totalCount: 1_000_000 });
    let cancel: ((reason: Error) => void) | undefined;
    queryItemsMock.mockImplementationOnce(() => new Promise((_resolve, reject) => { cancel = reject; }))
      .mockResolvedValueOnce({ ...page([item(100001)], 1_000_000), start: 100000 });
    supersedeWindowMock.mockImplementationOnce(async () => { cancel?.(new Error('query superseded')); });
    const first = gridController.requestWindowAt(999999);
    const latest = gridController.requestWindowAt(100500);
    await Promise.all([first, latest]);
    expect(supersedeWindowMock).toHaveBeenCalled();
    expect(queryItemsMock.mock.calls[queryItemsMock.mock.calls.length - 1][2].generation).toBeGreaterThan(queryItemsMock.mock.calls[0][2].generation);
    expect(store.get(gridItemsAtom).map(entry => entry.root_id)).toEqual([100001]);
    expect(store.get(gridErrorAtom)).toBeNull();
  });

  it('keeps loading while repeated scroll destinations remain inside the same window', async () => {
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), totalCount: 1_000_000 });
    let finish!: (value: EntityViewPage & { start: number }) => void;
    queryItemsMock.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
    supersedeWindowMock.mockClear();
    const pending = gridController.requestWindowAt(100500);
    for (let index = 100510; index < 100990; index += 10) {
      void gridController.requestWindowAt(index);
    }
    expect(queryItemsMock).toHaveBeenCalledOnce();
    expect(supersedeWindowMock).not.toHaveBeenCalled();
    finish({ ...page([item(100001)], 1_000_000), start: 100000 });
    await pending;
    expect(store.get(gridItemsAtom)[0].root_id).toBe(100001);
    expect(store.get(gridErrorAtom)).toBeNull();
  });

  it('removes affected stale rows and fills a complete loaded window from the tail', async () => {
    const loaded = Array.from({ length: 501 }, (_, index) => item(index + 1));
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      items: loaded,
      windowStart: 0,
      totalCount: 501,
    });
    queryItemsMock.mockResolvedValueOnce(page([...loaded.slice(1), item(502)], 501));

    await gridController.reconcile([1]);

    expect(queryItemsMock.mock.calls.map((call) => call[1])).toEqual([
      { position: { kind: 'ordinal', value: 0 }, limit: 501 },
    ]);
    expect(store.get(gridSessionAtom).items.map((entry) => entry.root_id)).not.toContain(1);
    const reconciled = store.get(gridSessionAtom).items;
    expect(reconciled[reconciled.length - 1]?.root_id).toBe(502);
  });

  it('drops an explicitly selected loaded root when reconciliation removes it', async () => {
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      items: [item(1), item(2)],
      queryHeadItems: [item(1), item(2)],
      totalCount: 2,
    });
    store.set(gridSelectionAtom, {
      ...emptyGridSelection(),
      itemIds: new Set([1, 2]),
      anchor: { kind: 'item', id: 1 },
    });
    queryItemsMock.mockResolvedValueOnce(page([item(2)], 1));

    await gridController.reconcile([1]);

    expect(store.get(gridSelectionAtom)).toEqual({
      ...emptyGridSelection(),
      itemIds: new Set([2]),
    });
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

  it.each(['ne', 'needle'])('keeps navigation alive when typing %s before it commits', async (text) => {
    vi.useFakeTimers();
    try {
      queryItemsMock.mockResolvedValue(page([item(9)], 1));
      const pending = gridController.prepareNavigation({ kind: 'folder', folder_id: 9 });
      gridController.setSearchText(text);
      const prepared = await pending;
      gridController.commitNavigation(prepared);
      await vi.advanceTimersByTimeAsync(250);
      expect(store.get(gridSessionAtom).scope).toEqual({ kind: 'folder', folder_id: 9 });
      expect(store.get(gridSessionAtom).searchText).toBe(text);
      expect(store.get(gridSessionAtom).status).toBe('idle');
      expect(queryText(queryItemsMock.mock.lastCall![0])).toBe(text.length >= 3 ? text : null);
    } finally { vi.useRealTimers(); }
  });

  it('does not discard an in-flight page for a below-minimum search change', async () => {
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), searchText: '' });
    let resolve!: (value: ReturnType<typeof page>) => void;
    queryItemsMock.mockReturnValueOnce(new Promise((done) => { resolve = done; }));
    const pending = gridController.loadFirstPage();
    gridController.setSearchText('ne');
    resolve(page([item(9)], 1));
    await pending;
    expect(store.get(gridSessionAtom).status).toBe('idle');
    expect(store.get(gridItemsAtom).map(item => item.root_id)).toEqual([9]);
  });

  it('settles text below three characters as an unfiltered query', async () => {
    vi.useFakeTimers();
    queryItemsMock
      .mockResolvedValueOnce(page([item(2)], 1))
      .mockResolvedValueOnce(page([item(3)], 1));

    gridController.setSearchText('cat');
    await vi.advanceTimersByTimeAsync(250);
    await vi.waitFor(() => expect(store.get(gridItemsAtom)).toEqual([item(2)]));

    gridController.setSearchText('ca');
    await vi.advanceTimersByTimeAsync(250);
    await vi.waitFor(() => expect(store.get(gridItemsAtom)).toEqual([item(3)]));

    expect(queryItemsMock).toHaveBeenCalledTimes(2);
    expect(queryText(queryItemsMock.mock.calls[1][0])).toBeNull();
    expect(store.get(gridTotalCountAtom)).toBe(1);
    expect(store.get(gridTotalSizeBytesAtom)).toBe(100);
    vi.useRealTimers();
  });

  it('does not re-query while short text leaves an unfiltered grid unchanged', async () => {
    vi.useFakeTimers();

    gridController.setSearchText('a');
    await vi.advanceTimersByTimeAsync(250);
    gridController.setSearchText('ab');
    await vi.advanceTimersByTimeAsync(250);

    expect(queryItemsMock).not.toHaveBeenCalled();
    expect(store.get(gridItemsAtom)).toEqual([item(1)]);
    expect(store.get(gridTotalCountAtom)).toBe(2);
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

    // The stale scan was superseded in the backend as soon as the query changed.
    expect(supersedeWindowMock).toHaveBeenCalled();
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

  it('displays a coherent first page before coalescing a refresh for newer writes', async () => {
    vi.useFakeTimers();
    revisionState.library = 5;
    queryItemsMock
      .mockResolvedValueOnce({ ...page([item(1)], 1), revision: 4 })
      .mockResolvedValueOnce({ ...page([item(2)], 1), revision: 5 });

    await gridController.loadFirstPage();
    expect(queryItemsMock).toHaveBeenCalledOnce();
    expect(store.get(gridSessionAtom).revision).toBe(4);
    expect(store.get(gridLoadingAtom)).toBe(false);
    await vi.advanceTimersByTimeAsync(40);
    expect(queryItemsMock).toHaveBeenCalledTimes(2);
    expect(store.get(gridItemsAtom).map((entry) => entry.root_id)).toEqual([2]);
    expect(store.get(gridSessionAtom).revision).toBe(5);
    vi.useRealTimers();
  });
  it('does not let an outgoing scroll or refresh cancel a pending destination', async () => {
    let resolveDestination: ((value: ReturnType<typeof page>) => void) | undefined;
    queryItemsMock.mockImplementationOnce(() => new Promise(resolve => { resolveDestination = resolve; }));
    const pending = gridController.prepareNavigation({ kind: 'inbox' });
    await vi.waitFor(() => expect(queryItemsMock).toHaveBeenCalledOnce());
    await gridController.requestWindowAt(999000);
    await gridController.reconcile();
    expect(queryItemsMock).toHaveBeenCalledOnce();
    resolveDestination?.(page([item(77)], 1));
    const prepared = await pending;
    expect(prepared.session.status).toBe('idle');
    gridController.commitNavigation(prepared);
    expect(store.get(gridSessionAtom).scope.kind).toBe('inbox');
    expect(store.get(gridItemsAtom)[0].root_id).toBe(77);
    gridController.cancelPreparation();
  });

  it('refreshes a second fast edit after 40 ms instead of keeping a stale grid for two seconds', async () => {
    vi.useFakeTimers();
    try {
      queryItemsMock
        .mockResolvedValueOnce({...page([item(2)], 2), revision: 2})
        .mockResolvedValueOnce({...page([item(3)], 3), revision: 3});
      await gridController.reconcile();
      gridController.requestReconcile();
      await vi.advanceTimersByTimeAsync(39);
      expect(queryItemsMock).toHaveBeenCalledOnce();
      await vi.advanceTimersByTimeAsync(1);
      expect(queryItemsMock).toHaveBeenCalledTimes(2);
      expect(store.get(gridTotalCountAtom)).toBe(3);
      expect(store.get(gridSessionAtom).revision).toBe(3);
      expect(store.get(gridLoadingAtom)).toBe(false);
    } finally { gridController.deactivate(); vi.useRealTimers(); }
  });

  it('coalesces continuous writes into one successor and yields in proportion to slow refresh work', async () => {
    vi.useFakeTimers();
    try {
      let finish: ((value: ReturnType<typeof page>) => void) | undefined;
      queryItemsMock
        .mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }))
        .mockResolvedValueOnce({...page([item(3)], 3), revision: 3});
      const pending = gridController.reconcile();
      for (let i = 0; i < 8; i++) {
        gridController.requestReconcile();
        await vi.advanceTimersByTimeAsync(10);
      }
      expect(queryItemsMock).toHaveBeenCalledOnce();
      expect(store.get(gridItemsAtom)[0].root_id).toBe(1);
      finish?.({...page([item(2)], 2), revision: 2});
      await pending;
      // An 80 ms refresh yields for 240 ms, even if more events arrive.
      for (let i = 0; i < 23; i++) {
        gridController.requestReconcile();
        await vi.advanceTimersByTimeAsync(10);
      }
      expect(queryItemsMock).toHaveBeenCalledOnce();
      await vi.advanceTimersByTimeAsync(10);
      expect(queryItemsMock).toHaveBeenCalledTimes(2);
      expect(store.get(gridSessionAtom).revision).toBe(3);
      await vi.advanceTimersByTimeAsync(2_000);
      expect(queryItemsMock).toHaveBeenCalledTimes(2);
    } finally { gridController.deactivate(); vi.useRealTimers(); }
  });
  describe('view preference saves', () => {
    const visit = async (folderId: number) => {
      const prepared = await gridController.prepareNavigation({ kind: 'folder', folder_id: folderId });
      gridController.commitNavigation(prepared);
    };

    it('saves each patch to the scope it was made in, never merging scopes', async () => {
      vi.useFakeTimers();
      try {
        await visit(1);
        vi.mocked(setViewPrefs).mockClear();
        gridController.saveViewPref({ spacing: 'tight' });
        await visit(2);
        gridController.saveViewPref({ target_size: 300 });
        await vi.advanceTimersByTimeAsync(501);
        expect(vi.mocked(setViewPrefs).mock.calls).toEqual([
          ['folder:1', { spacing: 'tight' }],
          ['folder:2', { target_size: 300 }],
        ]);
      } finally { gridController.deactivate(); vi.useRealTimers(); }
    });

    it('flushes pending saves on deactivate and drops them on library switch', async () => {
      vi.useFakeTimers();
      try {
        await visit(3);
        vi.mocked(setViewPrefs).mockClear();
        gridController.saveViewPref({ show_name: true });
        gridController.deactivate();
        expect(setViewPrefs).toHaveBeenCalledWith('folder:3', { show_name: true });

        await visit(4);
        vi.mocked(setViewPrefs).mockClear();
        gridController.saveViewPref({ show_name: false });
        gridController.discardPendingPreferences();
        await vi.advanceTimersByTimeAsync(1_000);
        expect(setViewPrefs).not.toHaveBeenCalled();
      } finally { gridController.deactivate(); vi.useRealTimers(); }
    });

    it('reports a failed save instead of swallowing it', async () => {
      vi.useFakeTimers();
      const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
      try {
        await visit(5);
        vi.mocked(setViewPrefs).mockRejectedValueOnce(new Error('disk full'));
        gridController.saveViewPref({ spacing: 'wide' });
        await vi.advanceTimersByTimeAsync(501);
        expect(warn).toHaveBeenCalledWith('Could not save grid view preferences', 'folder:5', expect.any(Error));
      } finally { warn.mockRestore(); gridController.deactivate(); vi.useRealTimers(); }
    });
  });
});
