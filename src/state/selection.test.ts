import { createStore } from 'jotai';
import { describe, expect, it } from 'vitest';
import type { ItemSummary } from '../shared/types/generated/application/ItemSummary';
import { currentGridQueryAtom, gridSessionAtom } from './grid';
import {
  clearSelectionAtom,
  emptyGridSelection,
  gridSelectionActionAtom,
  loadedSelectedItemIdsAtom,
  reduceGridSelection,
  selectedItemIdsAtom,
  selectedSubfolderNodeIdAtom,
  selectedSubfolderNodeIdsAtom,
  selectionCountAtom,
  selectionTargetAtom,
  selectAllResultsAtom,
} from './selection';

function buildGridItem(itemId: number, fileHash: string): ItemSummary {
  return {
    item_id: itemId,
    kind: 'media',
    lifecycle: 'active',
    label: null,
    name: 'Item',
    display_media_item_id: itemId,
    display_file_hash: fileHash,
    display_mime_type: 'image/jpeg',
    pixel_width: 100,
    pixel_height: 100,
    duration_ms: null,
    frame_count: null,
    has_audio: false,
    dominant_color_hex: null,
    size_bytes: 100,
    rating: null,
    captured_at: '2026-01-01T00:00:00Z',
    imported_at: '2026-01-01T00:00:00Z',
    media_count: 1,
  };
}

describe('selection state', () => {
  it('builds query targets from the canonical grid session', () => {
    const store = createStore();
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      scope: { kind: 'folder', folder_id: 4 },
      sort: { field: 'name', direction: 'ascending' },
      items: [buildGridItem(1, 'a'), buildGridItem(2, 'b')],
      totalCount: 12,
    });

    store.set(selectAllResultsAtom);

    expect(store.get(selectionCountAtom)).toBe(12);
    expect(store.get(selectionTargetAtom)).toEqual({
      kind: 'query',
      query: store.get(currentGridQueryAtom),
      excluded_item_ids: [],
    });
  });

  it('uses numeric item IDs for explicit targets and anchors', () => {
    const initial = emptyGridSelection();
    const selected = reduceGridSelection(initial, {
      type: 'replace_items', itemIds: new Set([11]), anchor: 11,
    });
    expect(selected.itemIds).toEqual(new Set([11]));
    expect(selected.anchor).toEqual({ kind: 'item', id: 11 });
    expect(reduceGridSelection(selected, { type: 'range_items', itemIds: new Set([11, 12]) }).itemIds)
      .toEqual(new Set([11, 12]));
  });

  it('keeps query-wide selection while excluding a numeric item', () => {
    const query = reduceGridSelection(emptyGridSelection(), { type: 'select_all', totalCount: 1_000_000 });
    const excluded = reduceGridSelection(query, { type: 'toggle_query_item', itemId: 42, totalCount: 1_000_000 });
    expect(excluded.mode).toBe('query_results');
    expect(excluded.excludedItemIds).toEqual(new Set([42]));
    expect(excluded.anchor).toEqual({ kind: 'item', id: 42 });
  });

  it('keeps item and folder marquee hits in separate sets', () => {
    const selection = reduceGridSelection(emptyGridSelection(), {
      type: 'marquee', itemIds: new Set([7]), folderNodeIds: new Set(['folder:2']), additive: false,
    });
    expect(selection.itemIds).toEqual(new Set([7]));
    expect(selection.folderNodeIds).toEqual(new Set(['folder:2']));
  });

  it('keeps subfolder selection out of item targets', () => {
    const store = createStore();
    store.set(selectedItemIdsAtom, new Set([1]));
    store.set(selectedSubfolderNodeIdsAtom, new Set(['folder:9']));

    expect(store.get(selectedSubfolderNodeIdAtom)).toBe('folder:9');
    expect(store.get(selectedItemIdsAtom).size).toBe(0);
    expect(store.get(selectionCountAtom)).toBe(0);
    expect(store.get(selectionTargetAtom)).toBeNull();

    store.set(clearSelectionAtom);
    expect(store.get(selectedSubfolderNodeIdAtom)).toBeNull();
  });

  it('projects query selection onto loaded numeric item IDs', () => {
    const store = createStore();
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      items: [buildGridItem(1, 'a'), buildGridItem(2, 'b')],
      totalCount: 4,
    });
    store.set(selectAllResultsAtom);
    store.set(gridSelectionActionAtom, { type: 'toggle_query_item', itemId: 2, totalCount: 4 });

    expect(store.get(loadedSelectedItemIdsAtom)).toEqual(new Set([1]));
  });
});
