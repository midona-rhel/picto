import { createStore } from 'jotai';
import { describe, expect, it } from 'vitest';
import type { CanonicalEntityGridItem } from '../shared/types/canonical';
import {
  currentGridQueryAtom,
  gridItemsAtom,
  gridScopeAtom,
  gridSortDirectionAtom,
  gridSortFieldAtom,
  gridTotalCountAtom,
} from './grid';
import {
  clearSelectionAtom,
  selectedEntityHashesAtom,
  selectedSubfolderNodeIdAtom,
  selectedSubfolderNodeIdsAtom,
  selectionCountAtom,
  selectionTargetAtom,
  selectAllResultsAtom,
} from './selection';

function buildGridItem(entityHash: string): CanonicalEntityGridItem {
  return {
    entity_id: 1,
    entity_hash: entityHash,
    thumbnail_hash: entityHash,
    entity_kind: 'single',
    name: 'Item',
    mime_type: 'image/jpeg',
    pixel_width: 100,
    pixel_height: 100,
    status: 1,
    rating: null,
    date_added: '2026-01-01T00:00:00Z',
    date_created: '2026-01-01T00:00:00Z',
    date_modified: '2026-01-01T00:00:00Z',
    has_thumbnail: true,
    member_count: null,
    duration_ms: null,
    frame_count: null,
    has_audio: false,
    dominant_color_hex: null,
    size_bytes: 100,
  };
}

describe('selection state', () => {
  it('builds query-results targets from canonical grid query state', () => {
    const store = createStore();
    store.set(gridScopeAtom, { kind: 'folder', id: 4 });
    store.set(gridSortFieldAtom, 'name');
    store.set(gridSortDirectionAtom, 'asc');
    store.set(gridItemsAtom, [buildGridItem('a'), buildGridItem('b')]);
    store.set(gridTotalCountAtom, 12);

    store.set(selectAllResultsAtom);

    expect(store.get(selectionCountAtom)).toBe(12);
    expect(store.get(selectionTargetAtom)).toEqual({
      kind: 'query_results',
      query: store.get(currentGridQueryAtom),
      excluded_entity_hashes: [],
    });
  });

  it('keeps subfolder tile selection out of entity selection targets', () => {
    const store = createStore();
    store.set(selectedEntityHashesAtom, new Set(['hash-1']));
    store.set(selectedSubfolderNodeIdsAtom, new Set(['folder:9']));

    expect(store.get(selectedSubfolderNodeIdAtom)).toBe('folder:9');
    expect(store.get(selectedEntityHashesAtom).size).toBe(0);
    expect(store.get(selectionCountAtom)).toBe(0);
    expect(store.get(selectionTargetAtom)).toBeNull();

    store.set(clearSelectionAtom);
    expect(store.get(selectedSubfolderNodeIdAtom)).toBeNull();
  });
});
