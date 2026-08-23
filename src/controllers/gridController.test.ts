import { getDefaultStore } from 'jotai';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { ItemPage } from '../shared/types/generated/application/ItemPage';
import type { ItemSummary } from '../shared/types/generated/application/ItemSummary';
import {
  gridCursorAtom,
  gridErrorAtom,
  gridItemsAtom,
  gridLoadingAtom,
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
    display_file_hash: `hash-${id}`,
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

function page(items: ItemSummary[], totalCount: number): ItemPage {
  return {
    items,
    revision: 1,
    visible_item_count: totalCount,
    visible_media_count: totalCount,
    total_size_bytes: totalCount * 100,
  };
}

describe('gridController pagination', () => {
  beforeEach(() => {
    queryItemsMock.mockReset();
    store.set(gridItemsAtom, [item(1)]);
    store.set(gridCursorAtom, 1);
    store.set(gridTotalCountAtom, 2);
    store.set(gridTotalSizeBytesAtom, 100);
    store.set(gridLoadingAtom, false);
    store.set(gridErrorAtom, null);
  });

  it('appends a page and updates canonical page metadata', async () => {
    queryItemsMock.mockResolvedValueOnce(page([item(2)], 2));
    await gridController.loadNextPage();

    expect(queryItemsMock).toHaveBeenCalledTimes(1);
    expect(queryItemsMock.mock.calls[0][1]).toEqual({ limit: 500, offset: 1 });
    expect(store.get(gridItemsAtom).map((entry) => entry.item_id)).toEqual([1, 2]);
    expect(store.get(gridCursorAtom)).toBeNull();
    expect(store.get(gridTotalCountAtom)).toBe(2);
    expect(store.get(gridTotalSizeBytesAtom)).toBe(200);
    expect(store.get(gridLoadingAtom)).toBe(false);
  });

  it('uses the same append path for background prefetch pages', async () => {
    store.set(gridTotalCountAtom, 3);
    queryItemsMock
      .mockResolvedValueOnce(page([item(2)], 3))
      .mockResolvedValueOnce(page([item(3)], 3));

    await gridController.prefetchToMinimum();

    expect(queryItemsMock.mock.calls.map((call) => call[1].offset)).toEqual([1, 2]);
    expect(store.get(gridItemsAtom).map((entry) => entry.item_id)).toEqual([1, 2, 3]);
    expect(store.get(gridErrorAtom)).toBeNull();
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

  it('keeps prefetch failures silent so foreground pagination can retry', async () => {
    queryItemsMock.mockRejectedValueOnce(new Error('prefetch failed'));
    await gridController.prefetchToMinimum();

    expect(store.get(gridCursorAtom)).toBe(1);
    expect(store.get(gridErrorAtom)).toBeNull();

    queryItemsMock.mockResolvedValueOnce(page([item(2)], 2));
    await gridController.loadNextPage();
    expect(queryItemsMock).toHaveBeenCalledTimes(2);
  });
});
