import { getDefaultStore } from 'jotai';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { CanonicalEntityGridItem, EntityViewPage } from '../shared/types/canonical';
import {
  gridCursorAtom,
  gridErrorAtom,
  gridItemsAtom,
  gridLoadingAtom,
  gridSessionAtom,
  gridTotalCountAtom,
  gridTotalSizeBytesAtom,
} from '../state/grid';

const { queryEntityViewMock } = vi.hoisted(() => ({ queryEntityViewMock: vi.fn() }));

vi.mock('../platform/entityApi', () => ({
  queryEntityView: queryEntityViewMock,
  reconcileEntityView: vi.fn(),
}));

vi.mock('../platform/settingsApi', () => ({
  getViewPrefs: vi.fn().mockResolvedValue(null),
  setViewPrefs: vi.fn().mockResolvedValue(undefined),
}));

import { gridController } from './gridController';

const store = getDefaultStore();

function item(id: number): CanonicalEntityGridItem {
  return {
    entity_id: id,
    entity_hash: `hash-${id}`,
    name: `Item ${id}`,
    mime_type: 'image/jpeg',
    pixel_width: 100,
    pixel_height: 100,
    status: 1,
    rating: null,
    date_added: '2026-01-01T00:00:00Z',
    date_created: '2026-01-01T00:00:00Z',
    date_modified: '2026-01-01T00:00:00Z',
    has_thumbnail: true,
    duration_ms: null,
    frame_count: null,
    has_audio: false,
    dominant_color_hex: null,
    size_bytes: 100,
  };
}

function page(items: CanonicalEntityGridItem[], nextCursor: string | null, totalCount: number): EntityViewPage {
  return {
    items,
    next_cursor: nextCursor,
    total_count: totalCount,
    total_size_bytes: totalCount * 100,
  };
}

describe('gridController pagination', () => {
  beforeEach(() => {
    queryEntityViewMock.mockReset();
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      items: [item(1)], cursor: 'cursor-1', totalCount: 2,
      totalSizeBytes: 100, status: 'idle', error: null, generation: 0,
    });
  });

  it('appends a scroll-requested page and updates canonical page metadata', async () => {
    queryEntityViewMock.mockResolvedValueOnce(page([item(2)], null, 2));

    await gridController.loadNextPage();

    expect(queryEntityViewMock).toHaveBeenCalledTimes(1);
    expect(queryEntityViewMock.mock.calls[0][0].page).toEqual({ limit: 500, cursor: 'cursor-1' });
    expect(store.get(gridItemsAtom).map((entry) => entry.entity_hash)).toEqual(['hash-1', 'hash-2']);
    expect(store.get(gridCursorAtom)).toBeNull();
    expect(store.get(gridTotalCountAtom)).toBe(2);
    expect(store.get(gridTotalSizeBytesAtom)).toBe(200);
    expect(store.get(gridLoadingAtom)).toBe(false);
  });

  it('uses one guarded append path for sequential pages', async () => {
    store.set(gridSessionAtom, { ...store.get(gridSessionAtom), totalCount: 3 });
    queryEntityViewMock
      .mockResolvedValueOnce(page([item(2)], 'cursor-2', 3))
      .mockResolvedValueOnce(page([item(3)], null, 3));

    await gridController.loadNextPage();
    await gridController.loadNextPage();

    expect(queryEntityViewMock).toHaveBeenCalledTimes(2);
    expect(queryEntityViewMock.mock.calls.map((call) => call[0].page.cursor)).toEqual(['cursor-1', 'cursor-2']);
    expect(store.get(gridItemsAtom).map((entry) => entry.entity_hash)).toEqual(['hash-1', 'hash-2', 'hash-3']);
    expect(store.get(gridErrorAtom)).toBeNull();
    expect(store.get(gridLoadingAtom)).toBe(false);
  });

  it('deduplicates concurrent requests for the same cursor', async () => {
    let resolvePage: ((value: EntityViewPage) => void) | undefined;
    queryEntityViewMock.mockImplementationOnce(() => new Promise<EntityViewPage>((resolve) => { resolvePage = resolve; }));

    const first = gridController.loadNextPage();
    const second = gridController.loadNextPage();
    resolvePage?.(page([item(2)], null, 2));
    await Promise.all([first, second]);

    expect(queryEntityViewMock).toHaveBeenCalledTimes(1);
    expect(store.get(gridItemsAtom).map((entry) => entry.entity_hash)).toEqual(['hash-1', 'hash-2']);
  });

  it('keeps the cursor after an append failure so pagination can retry', async () => {
    queryEntityViewMock.mockRejectedValueOnce(new Error('prefetch failed'));

    await gridController.loadNextPage();

    expect(store.get(gridCursorAtom)).toBe('cursor-1');
    expect(store.get(gridErrorAtom)).toBe('prefetch failed');
    expect(store.get(gridLoadingAtom)).toBe(false);

    queryEntityViewMock.mockResolvedValueOnce(page([item(2)], null, 2));
    await gridController.loadNextPage();
    expect(queryEntityViewMock).toHaveBeenCalledTimes(2);
  });
});
