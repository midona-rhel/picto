import { createStore } from 'jotai';
import { describe, expect, it } from 'vitest';
import type { CanonicalEntityGridItem } from '../shared/types/canonical';
import { currentGridQueryAtom, gridSessionAtom } from './grid';
import {
  clearSelectionAtom,
  emptyGridSelection,
  gridSelectionActionAtom,
  gridSelectionAtom,
  loadedSelectedItemIdsAtom,
  reduceGridSelection,
  selectedItemIdsAtom,
  selectedSubfolderNodeIdAtom,
  selectedSubfolderNodeIdsAtom,
  selectionCountAtom,
  selectionTargetAtom,
  selectAllResultsAtom,
} from './selection';

function buildGridItem(itemId: number, fileHash: string): CanonicalEntityGridItem {
  return {
    root_id: itemId,
    kind: 'media',
    lifecycle: 'active',
    name: 'Item',
    cover_media_id: itemId,
    content_hash: fileHash,
    mime: 'image/jpeg',
    width: 100,
    height: 100,
    duration_ms: null,
    frame_count: null,
    palette: [],
    imported_at_ms: itemId,
    captured_at_ms: null,
    modified_at_ms: itemId,
    rating: 'unrated',
    media_count: 1,
    total_size_bytes: 100,
  };
}

describe('selection state', () => {
  it('keeps a shift-click range with an unloaded anchor as a backend range', () => {
    const store = createStore();
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      items: [buildGridItem(1, 'a'), buildGridItem(2, 'b'), buildGridItem(3, 'c')],
      windowStart: 5_000,
      totalCount: 1_000_000,
    });
    const query = store.get(currentGridQueryAtom);
    store.set(gridSelectionActionAtom, {
      type: 'range_query',
      query,
      anchor: { id: 11, index: 20 },
      focus: { id: 2, index: 5_001 },
    });

    expect(store.get(selectionCountAtom)).toBe(4_982);
    // Loaded tiles light up only up to the focus position.
    expect(store.get(loadedSelectedItemIdsAtom)).toEqual(new Set([1, 2]));
    store.set(gridSelectionActionAtom, { type: 'toggle_query_item', itemId: 1, totalCount: 1_000_000, index: 5_000 });
    expect(store.get(selectionCountAtom)).toBe(4_981);
    // A tile beyond the range is added, not excluded (#78).
    store.set(gridSelectionActionAtom, { type: 'toggle_query_item', itemId: 3, totalCount: 1_000_000, index: 5_002 });
    expect(store.get(selectionCountAtom)).toBe(4_982);
    expect(store.get(loadedSelectedItemIdsAtom)).toEqual(new Set([2, 3]));
    expect(store.get(selectionTargetAtom)).toEqual({
      kind: 'range',
      query,
      spans: [{ anchor_root_id: 11, focus_root_id: 2, anchor_index: 20, focus_index: 5_001 }],
      included_root_ids: [3],
      excluded_root_ids: [1],
    });
    // Toggling inside a range keeps the original anchor for the next shift-click.
    expect(store.get(gridSelectionAtom).anchor).toEqual({ kind: 'item', id: 11, index: 20 });
  });

  it('adds an off-window range to a selection without losing unloaded roots (#78)', () => {
    const store = createStore();
    const query = store.get(currentGridQueryAtom);
    store.set(gridSelectionActionAtom, { type: 'replace_items', itemIds: new Set([7]), anchor: 7, anchorIndex: 10 });
    store.set(gridSelectionActionAtom, { type: 'toggle_item', itemId: 8, index: 9_000 });
    // Cmd+Shift from root 8 (index 9,000) back to root 50 (index 5,010).
    store.set(gridSelectionActionAtom, {
      type: 'range_add',
      query,
      span: { anchor: { id: 8, index: 9_000 }, focus: { id: 50, index: 5_010 } },
      positions: new Map([[7, 10], [8, 9_000]]),
    });
    // 3,991 in the span plus root 7, which lies outside it.
    expect(store.get(selectionCountAtom)).toBe(3_992);
    expect(store.get(selectionTargetAtom)).toMatchObject({
      kind: 'range',
      included_root_ids: [7],
      excluded_root_ids: [],
    });
  });

  it('follows backend positions after the view changes, and drops a span that lost an end (#79)', () => {
    const store = createStore();
    const query = store.get(currentGridQueryAtom);
    store.set(gridSelectionActionAtom, {
      type: 'range_query', query, anchor: { id: 1, index: 0 }, focus: { id: 5, index: 4 },
    });
    expect(store.get(selectionCountAtom)).toBe(5);
    // The view reorders to [2, 1, 5, 3, 4]: the backend now spans roots 1..5 = positions 1..2.
    store.set(gridSelectionActionAtom, { type: 'range_reconcile', positions: new Map([[1, 1], [5, 2]]) });
    expect(store.get(selectionCountAtom)).toBe(2);
    expect(store.get(selectionTargetAtom)).toMatchObject({
      spans: [{ anchor_root_id: 1, focus_root_id: 5, anchor_index: 1, focus_index: 2 }],
    });
    // Root 5 leaves the view: nothing describes that run any more.
    store.set(gridSelectionActionAtom, { type: 'range_reconcile', positions: new Map([[1, 1], [5, null]]) });
    expect(store.get(gridSelectionAtom).range ?? null).toBeNull();
    expect(store.get(selectionCountAtom)).toBe(0);
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
      excluded_root_ids: [],
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
