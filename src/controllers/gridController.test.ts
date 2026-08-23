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
  gridSessionAtom,
  gridTotalCountAtom,
  gridTotalSizeBytesAtom,
} from '../state/grid';

const { queryItemsMock } = vi.hoisted(() => ({ queryItemsMock: vi.fn() }));

vi.mock('../platform/entityApi', () => ({ queryItems: queryItemsMock }));
vi.mock('../platform/settingsApi', () => ({
  getViewPrefs: vi.fn().mockResolvedValue(null),
  setViewPrefs: vi.fn().mockResolvedValue(undefined),
}));

import { gridController } from './gridController';

const store = getDefaultStore();

function item(id: number): ItemSummary {
  return {
    item_id: id,
    kind: 'media',
    lifecycle: 'active',
    label: null,
    name: `Item ${id}`,
    display_media_item_id: id,
    display_file_hash: `file-${id}`,
    display_mime_type: 'image/jpeg',
    pixel_width: 100,
    pixel_height: 100,
    duration_ms: null,
    frame_count: null,
    has_audio: false,
    dominant_color_hex: null,
    size_bytes: 100,
    rating: null,
    captured_at: null,
    imported_at: '2026-01-01T00:00:00Z',
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

describe('gridController pagination', () => {
  beforeEach(() => {
    queryItemsMock.mockReset();
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      items: [item(1)],
      cursor: 1,
      totalCount: 2,
      totalSizeBytes: 100,
      status: 'idle',
      error: null,
      generation: 0,
    });
  });

  it('queries the replacement page contract and appends by item_id', async () => {
    queryItemsMock.mockResolvedValueOnce(page([item(2)], 2));

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
      .mockResolvedValueOnce(page([item(2)], 3))
      .mockResolvedValueOnce(page([item(3)], 3));

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
      filters: { include_tags: [], exclude_tags: [], minimum_rating: null, mime_prefix: null, text: null },
      sort: { field: 'imported_at', direction: 'descending', random_seed: null },
    });
    expect('entity_hash' in query).toBe(false);
    expect('page' in query).toBe(false);
  });
});
