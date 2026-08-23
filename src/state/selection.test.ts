import { createStore } from 'jotai';
import { describe, expect, it } from 'vitest';
import type { CanonicalEntityGridItem } from '../shared/types/canonical';
import {
  currentGridQueryAtom,
  gridSessionAtom,
} from './grid';
import {
  clearSelectionAtom,
  emptyGridSelection,
  gridSelectionAtom,
  loadedSelectedEntityHashesAtom,
  reduceGridSelection,
  selectedFolderNodeIdAtom,
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
      query: {
        base_scope: { kind: 'folder', id: 4 },
        sort: { field: 'name', direction: 'asc' },
      },
      pages: [{
        items: [buildGridItem('a'), buildGridItem('b')],
        next_cursor: null,
        total_count: 12,
        total_size_bytes: 200,
      }],
    });

    store.set(selectAllResultsAtom);

    expect(store.get(selectionCountAtom)).toBe(12);
    expect(store.get(selectionTargetAtom)).toEqual({
      kind: 'query_results',
      query: store.get(currentGridQueryAtom),
      excluded_entity_hashes: [],
    });
  });

  it('represents a real mixed selection while entity targets stay canonical', () => {
    const store = createStore();
    store.set(gridSelectionAtom, {
      ...emptyGridSelection(),
      entityHashes: new Set(['hash-1']),
      folderNodeIds: new Set(['folder:9']),
    });

    expect(store.get(selectedFolderNodeIdAtom)).toBe('folder:9');
    expect(store.get(loadedSelectedEntityHashesAtom)).toEqual(new Set(['hash-1']));
    expect(store.get(selectionCountAtom)).toBe(1);
    expect(store.get(selectionTargetAtom)).toEqual({ kind: 'entity_hashes', entity_hashes: ['hash-1'] });

    store.set(clearSelectionAtom);
    expect(store.get(selectedFolderNodeIdAtom)).toBeNull();
  });

  it('keeps a hash anchor across range replacement and metadata replacement', () => {
    const initial = reduceGridSelection(emptyGridSelection(), {
      type: 'replace_entities', hashes: new Set(['a']), anchor: 'a',
    });
    const ranged = reduceGridSelection(initial, { type: 'range_entities', hashes: new Set(['a', 'b']) });
    expect(ranged.anchor).toEqual({ kind: 'entity', id: 'a' });
  });

  it('does not collapse query-wide selection when a folder is modifier-selected', () => {
    const query = reduceGridSelection(emptyGridSelection(), { type: 'select_all', totalCount: 1_000_000 });
    const mixed = reduceGridSelection(query, { type: 'toggle_folder', id: 'folder:4' });
    const excluded = reduceGridSelection(mixed, { type: 'toggle_query_entity', hash: 'visible-a', totalCount: 1_000_000 });
    expect(excluded.mode).toBe('query_results');
    expect(excluded.folderNodeIds).toEqual(new Set(['folder:4']));
    expect(excluded.excludedEntityHashes).toEqual(new Set(['visible-a']));
  });

  it('keeps entity and folder marquee hits in separate sets', () => {
    const selection = reduceGridSelection(emptyGridSelection(), {
      type: 'marquee', entityHashes: new Set(['hash-a']), folderNodeIds: new Set(['folder:2']), additive: false,
    });
    expect(selection.entityHashes).toEqual(new Set(['hash-a']));
    expect(selection.folderNodeIds).toEqual(new Set(['folder:2']));
  });
});
