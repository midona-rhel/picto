import { createStore } from 'jotai';
import { describe, expect, it } from 'vitest';
import type { ItemSummary } from '../shared/types/generated/application/ItemSummary';
import type { ItemTarget } from '../shared/types/generated/application/ItemTarget';
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
    name: 'Item',
    display_file_hash: fileHash,
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

describe('selection state', () => {
  it('exposes range targets without expanding unloaded renderer item IDs', () => {
    const query = createStore().get(currentGridQueryAtom);
    const target: ItemTarget = {
      kind: 'range',
      query,
      anchor_item_id: 11,
      focus_item_id: 999_999,
    };

    expect(target).toEqual({
      kind: 'range',
      query,
      anchor_item_id: 11,
      focus_item_id: 999_999,
    });
    expect('item_ids' in target).toBe(false);
  });

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

  it('does not mix folder and media marquee selection', () => {
    const selection = reduceGridSelection(emptyGridSelection(), {
      type: 'marquee', itemIds: new Set([7]), folderNodeIds: new Set(['folder:2']), additive: false,
    });
    expect(selection.itemIds).toEqual(new Set());
    expect(selection.folderNodeIds).toEqual(new Set(['folder:2']));
  });

  it('switches selection domains instead of mixing folders and media', () => {
    const itemSelection = reduceGridSelection(emptyGridSelection(), {
      type: 'replace_items', itemIds: new Set([7]), anchor: 7,
    });
    const folderSelection = reduceGridSelection(itemSelection, { type: 'toggle_folder', id: 'folder:2' });
    expect(folderSelection.itemIds).toEqual(new Set());
    expect(folderSelection.folderNodeIds).toEqual(new Set(['folder:2']));

    const nextItemSelection = reduceGridSelection(folderSelection, { type: 'toggle_item', itemId: 8 });
    expect(nextItemSelection.itemIds).toEqual(new Set([8]));
    expect(nextItemSelection.folderNodeIds).toEqual(new Set());
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
