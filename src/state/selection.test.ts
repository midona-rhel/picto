import { createStore } from 'jotai';
import { describe, expect, it } from 'vitest';
import type { ItemSummary } from '../shared/types/generated/application/ItemSummary';
import { currentGridQueryAtom, gridSessionAtom } from './grid';
import {
  clearSelectionAtom,
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
});
