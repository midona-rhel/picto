import { getDefaultStore } from 'jotai';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ItemPage } from '../shared/types/generated/application/ItemPage';
import type { ItemSummary } from '../shared/types/generated/application/ItemSummary';
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
import { gridSelectionAtom } from '../state/selection';

const { queryItemsMock, getViewPrefsMock } = vi.hoisted(() => ({
  queryItemsMock: vi.fn(),
  getViewPrefsMock: vi.fn(),
}));

vi.mock('../platform/entityApi', () => ({ queryItems: queryItemsMock }));
vi.mock('../platform/settingsApi', () => ({
  GRID_DEFAULTS_SCOPE: 'grid:defaults',
  getViewPrefs: getViewPrefsMock,
  setViewPrefs: vi.fn().mockResolvedValue(undefined),
}));

import { gridController } from './gridController';

const store = getDefaultStore();

function item(id: number): ItemSummary {
  return {
    item_id: id,
    kind: 'media',
    lifecycle: 'active',
    name: `Item ${id}`,
    display_file_hash: `file-${id}`,
    display_mime_type: 'image/jpeg',
    pixel_width: 100,
    pixel_height: 100,
    duration_ms: null,
    frame_count: null,
    dominant_color_hex: null,
    rating: null,
    media_count: 1,
  };
}

function page(items: ItemSummary[], visibleCount: number): ItemPage {
  return {
    items,
    revision: 1,
    visible_item_count: visibleCount,
    visible_media_count: visibleCount,
    total_size_bytes: visibleCount * 100,
  };
}

function appendPage(items: ItemSummary[]): ItemPage {
  return {
    items,
    revision: 1,
    visible_item_count: null,
    visible_media_count: null,
    total_size_bytes: null,
  };
}

describe('gridController pagination', () => {
  beforeEach(() => {
    queryItemsMock.mockReset();
    getViewPrefsMock.mockReset();
    getViewPrefsMock.mockResolvedValue({});
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      scope: { kind: 'all' },
      sort: { field: 'imported_at', direction: 'descending' },
      items: [item(1)],
      cursor: 1,
      totalCount: 2,
      totalSizeBytes: 200,
      status: 'idle',
      error: null,
      generation: 0,
    });
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
    let resolvePage: ((value: ItemPage) => void) | undefined;
    queryItemsMock.mockImplementationOnce(() => new Promise<ItemPage>((resolve) => {
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
    expect(store.get(gridItemsAtom).map((entry) => entry.item_id)).toEqual([1]);
    expect(store.get(gridTotalCountAtom)).toBe(2);
    expect(store.get(gridSessionAtom).view).toEqual(expect.objectContaining({ mode: 'waterfall' }));
    expect(store.get(gridSelectionAtom).itemIds).toEqual(new Set([1]));

    resolvePage?.(page([item(7)], 1));
    const prepared = await navigation;
    expect(store.get(gridSessionAtom).scope).toEqual({ kind: 'all' });
    expect(store.get(gridItemsAtom).map((entry) => entry.item_id)).toEqual([1]);
    expect(store.get(gridSessionAtom).view.mode).toBe('waterfall');
    expect(prepared.session.view).toEqual(expect.objectContaining({
      mode: 'grid',
      targetSize: 333,
      showSubfolders: false,
    }));

    gridController.commitNavigation(prepared);
    expect(store.get(gridSessionAtom).scope).toEqual({ kind: 'folder', folder_id: 7 });
    expect(store.get(gridItemsAtom).map((entry) => entry.item_id)).toEqual([7]);
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

    expect(queryItemsMock.mock.calls[0][0].sort).toEqual({
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

    expect(queryItemsMock.mock.calls[0][0].sort).toEqual({
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

  it('keeps Inbox oldest first despite global or saved scope preferences', async () => {
    getViewPrefsMock.mockImplementation(async (scope: string) => scope === 'grid:defaults'
      ? { sort_field: 'name', sort_order: 'descending' }
      : { sort_field: 'rating', sort_order: 'descending' });
    queryItemsMock.mockResolvedValueOnce(page([item(1)], 1));

    await gridController.navigateTo({ kind: 'inbox' }, {
      sort: { field: 'name', direction: 'descending', random_seed: null },
    });

    expect(queryItemsMock.mock.calls[0][0].sort).toEqual({
      field: 'imported_at',
      direction: 'ascending',
      random_seed: null,
    });

    queryItemsMock.mockResolvedValueOnce(page([item(1)], 1));

    await gridController.navigateTo({ kind: 'inbox' });

    expect(queryItemsMock.mock.calls[1][0].sort).toEqual({
      field: 'imported_at',
      direction: 'ascending',
      random_seed: null,
    });
  });

  it('queries the replacement page contract and appends by item_id', async () => {
    queryItemsMock.mockResolvedValueOnce(appendPage([item(2)]));

    await gridController.loadNextPage();

    expect(queryItemsMock).toHaveBeenCalledTimes(1);
    expect(queryItemsMock.mock.calls[0][1]).toEqual({ offset: 1, limit: 500 });
    expect(store.get(gridItemsAtom).map((entry) => entry.item_id)).toEqual([1, 2]);
    expect(store.get(gridCursorAtom)).toBeNull();
    expect(store.get(gridTotalCountAtom)).toBe(2);
    expect(store.get(gridTotalSizeBytesAtom)).toBe(200);
    expect(store.get(gridLoadingAtom)).toBe(false);
  });

  it('uses one guarded append path for sequential pages', async () => {
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), totalCount: 3 });
    queryItemsMock
      .mockResolvedValueOnce(appendPage([item(2)]))
      .mockResolvedValueOnce(appendPage([item(3)]));

    await gridController.loadNextPage();
    await gridController.loadNextPage();

    expect(queryItemsMock).toHaveBeenCalledTimes(2);
    expect(queryItemsMock.mock.calls.map((call) => call[1].offset)).toEqual([1, 2]);
    expect(store.get(gridItemsAtom).map((entry) => entry.item_id)).toEqual([1, 2, 3]);
    expect(store.get(gridErrorAtom)).toBeNull();
    expect(store.get(gridLoadingAtom)).toBe(false);
  });

  it('deduplicates concurrent requests for the same offset', async () => {
    let resolvePage: ((value: ItemPage) => void) | undefined;
    queryItemsMock.mockImplementationOnce(() => new Promise<ItemPage>((resolve) => { resolvePage = resolve; }));

    const first = gridController.loadNextPage();
    const second = gridController.loadNextPage();
    resolvePage?.(page([item(2)], 2));
    await Promise.all([first, second]);

    expect(queryItemsMock).toHaveBeenCalledTimes(1);
    expect(store.get(gridItemsAtom).map((entry) => entry.item_id)).toEqual([1, 2]);
  });

  it('keeps the offset after an append failure so pagination can retry', async () => {
    queryItemsMock.mockRejectedValueOnce(new Error('page failed'));

    await gridController.loadNextPage();

    expect(store.get(gridCursorAtom)).toBe(1);
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
    expect(query).toMatchObject({
      scope: { kind: 'all' },
      filters: {
        include_tags: [],
        exclude_tags: [],
        ratings: [],
        include_mime_types: [],
        exclude_mime_types: [],
        text: null,
        color_hex: null,
      },
      sort: { field: 'imported_at', direction: 'descending', random_seed: null },
    });
    expect('entity_hash' in query).toBe(false);
    expect('page' in query).toBe(false);
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
    let resolvePage: ((value: ItemPage) => void) | undefined;
    queryItemsMock.mockImplementationOnce(() => new Promise<ItemPage>((resolve) => {
      resolvePage = resolve;
    }));

    const reconcile = gridController.reconcile();
    expect(store.get(gridSessionAtom).status).toBe('idle');
    expect(store.get(gridSessionAtom).generation).toBe(4);

    resolvePage?.(page([{ ...first }, { ...second }, item(3)], 3));
    await expect(reconcile).resolves.toBe(true);
    const after = store.get(gridSessionAtom);
    expect(queryItemsMock.mock.calls[0][1]).toEqual({ offset: 0, limit: 500 });
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
      cursor: 750,
      totalCount: 900,
    });
    queryItemsMock.mockResolvedValueOnce(page(loaded.slice(0, 500).map((entry) => ({ ...entry })), 900));

    await gridController.reconcile();

    expect(queryItemsMock.mock.calls[0][1]).toEqual({ offset: 0, limit: 500 });
    expect(store.get(gridSessionAtom).items).toHaveLength(750);
    expect(store.get(gridSessionAtom).items[749]).toBe(loaded[749]);
    expect(store.get(gridCursorAtom)).toBe(750);
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
      .mockResolvedValueOnce(page(loaded.slice(1, 501), 501))
      .mockResolvedValueOnce(appendPage([item(502)]));

    await gridController.reconcile([1]);

    expect(queryItemsMock.mock.calls.map((call) => call[1])).toEqual([
      { offset: 0, limit: 500 },
      { offset: 500, limit: 1 },
    ]);
    expect(store.get(gridSessionAtom).items.map((entry) => entry.item_id)).not.toContain(1);
    const reconciled = store.get(gridSessionAtom).items;
    expect(reconciled[reconciled.length - 1]?.item_id).toBe(502);
  });

  it('settles search after 100 ms of inactivity', async () => {
    vi.useFakeTimers();
    queryItemsMock.mockResolvedValueOnce(page([item(1)], 1));

    gridController.setSearchText('alice');
    await vi.advanceTimersByTimeAsync(99);
    expect(queryItemsMock).not.toHaveBeenCalled();

    await vi.advanceTimersByTimeAsync(1);
    expect(queryItemsMock).toHaveBeenCalledOnce();
    expect(queryItemsMock.mock.calls[0][0].filters.text).toBe('alice');
    vi.useRealTimers();
  });
});
