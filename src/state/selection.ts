/**
 * Selection state — explicit hash selection or full query-results selection.
 *
 * This keeps one canonical frontend target model aligned with the backend:
 * - explicit entity hashes
 * - current query results plus excluded hashes
 */

import { atom } from 'jotai';
import type { EntityTarget } from '../shared/types/canonical';
import { currentGridQueryAtom, gridItemsAtom, gridTotalCountAtom } from './grid';

export type SelectionMode = 'explicit' | 'query_results';

const selectionModeStateAtom = atom<SelectionMode>('explicit');
const explicitSelectionHashesAtom = atom<Set<string>>(new Set<string>());
const querySelectionExcludedHashesAtom = atom<Set<string>>(new Set<string>());

export const selectionModeAtom = atom((get) => get(selectionModeStateAtom));
export const querySelectionActiveAtom = atom((get) => get(selectionModeStateAtom) === 'query_results');
export const querySelectionExcludedEntityHashesAtom = atom((get) => get(querySelectionExcludedHashesAtom));

/**
 * The visible selected hashes in the current loaded grid window.
 * For query-results selection, this means "all loaded items except exclusions".
 */
export const selectedEntityHashesAtom = atom(
  (get) => {
    if (get(selectionModeStateAtom) === 'query_results') {
      const excluded = get(querySelectionExcludedHashesAtom);
      const selected = new Set<string>();
      for (const item of get(gridItemsAtom)) {
        if (!excluded.has(item.entity_hash)) {
          selected.add(item.entity_hash);
        }
      }
      return selected;
    }
    return get(explicitSelectionHashesAtom);
  },
  (get, set, update: Set<string> | ((prev: Set<string>) => Set<string>)) => {
    const prev = get(explicitSelectionHashesAtom);
    const next = typeof update === 'function' ? update(new Set(prev)) : new Set(update);
    set(selectionModeStateAtom, 'explicit');
    set(querySelectionExcludedHashesAtom, new Set<string>());
    set(explicitSelectionHashesAtom, next);
  },
);

export const clearSelectionAtom = atom(null, (_get, set) => {
  set(selectionModeStateAtom, 'explicit');
  set(explicitSelectionHashesAtom, new Set<string>());
  set(querySelectionExcludedHashesAtom, new Set<string>());
});

export const selectAllResultsAtom = atom(null, (get, set) => {
  const totalCount = get(gridTotalCountAtom) ?? get(gridItemsAtom).length;
  if (totalCount <= 0) {
    set(clearSelectionAtom);
    return;
  }
  set(selectionModeStateAtom, 'query_results');
  set(explicitSelectionHashesAtom, new Set<string>());
  set(querySelectionExcludedHashesAtom, new Set<string>());
});

export const toggleQuerySelectionHashAtom = atom(null, (get, set, entityHash: string) => {
  if (get(selectionModeStateAtom) !== 'query_results') {
    return;
  }
  const excluded = new Set(get(querySelectionExcludedHashesAtom));
  if (excluded.has(entityHash)) {
    excluded.delete(entityHash);
  } else {
    excluded.add(entityHash);
  }
  const totalCount = get(gridTotalCountAtom) ?? get(gridItemsAtom).length;
  if (excluded.size >= totalCount) {
    set(clearSelectionAtom);
    return;
  }
  set(querySelectionExcludedHashesAtom, excluded);
});

export const selectionCountAtom = atom((get) => {
  if (get(selectionModeStateAtom) === 'query_results') {
    const totalCount = get(gridTotalCountAtom) ?? get(gridItemsAtom).length;
    const count = totalCount - get(querySelectionExcludedHashesAtom).size;
    return count > 0 ? count : 0;
  }
  return get(explicitSelectionHashesAtom).size;
});

export const hasSelectionAtom = atom((get) => get(selectionCountAtom) > 0);

/** Single selected hash only exists for explicit single selection. */
export const selectedEntityHashAtom = atom((get) => {
  if (get(selectionModeStateAtom) !== 'explicit') return null;
  const set = get(explicitSelectionHashesAtom);
  if (set.size === 1) return set.values().next().value as string;
  return null;
});

export const selectionTargetAtom = atom<EntityTarget | null>((get) => {
  const count = get(selectionCountAtom);
  if (count <= 0) return null;
  if (get(selectionModeStateAtom) === 'query_results') {
    return {
      kind: 'query_results',
      query: get(currentGridQueryAtom),
      excluded_entity_hashes: Array.from(get(querySelectionExcludedHashesAtom)),
    };
  }
  return {
    kind: 'entity_hashes',
    entity_hashes: Array.from(get(explicitSelectionHashesAtom)),
  };
});
