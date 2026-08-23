import { createStore } from 'jotai';
import { describe, expect, it } from 'vitest';
import type { CanonicalEntityGridItem } from '../shared/types/canonical';
import {
  currentGridQueryAtom,
  gridSessionAtom,
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
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      scope: { kind: 'folder', id: 4 },
      sort: { field: 'name', direction: 'asc' },
      items: [buildGridItem('a'), buildGridItem('b')],
      totalCount: 12,
    });

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
