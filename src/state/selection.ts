/**
 * Selection state — explicit entity selection, virtual query-results selection,
 * and separate subfolder-tile selection for the header strip.
 *
 * Entity actions always operate on canonical EntityTarget values.
 * Subfolder tile selection is scope-only UI state and never becomes an EntityTarget.
 */

import { atom } from 'jotai';
import type { EntityTarget } from '../shared/types/canonical';
import { currentGridQueryAtom, gridItemsAtom, gridTotalCountAtom } from './grid';

export type SelectionMode = 'explicit' | 'query_results';

const selectionModeStateAtom = atom<SelectionMode>('explicit');
const explicitSelectionHashesAtom = atom<Set<string>>(new Set<string>());
const querySelectionExcludedHashesAtom = atom<Set<string>>(new Set<string>());
const subfolderSelectionNodeIdsStateAtom = atom<Set<string>>(new Set<string>());

export const selectionModeAtom = atom((get) => get(selectionModeStateAtom));
export const querySelectionActiveAtom = atom((get) => get(selectionModeStateAtom) === 'query_results');
export const querySelectionExcludedEntityHashesAtom = atom((get) => get(querySelectionExcludedHashesAtom));

export const selectedSubfolderNodeIdsAtom = atom<
  Set<string>,
  [Set<string> | ((prev: Set<string>) => Set<string>)],
  void
>(
  (get) => get(subfolderSelectionNodeIdsStateAtom),
  (get, set, update) => {
    const prev = get(subfolderSelectionNodeIdsStateAtom);
    const next = typeof update === 'function' ? update(new Set(prev)) : new Set(update);
    set(selectionModeStateAtom, 'explicit');
    set(explicitSelectionHashesAtom, new Set<string>());
    set(querySelectionExcludedHashesAtom, new Set<string>());
    set(subfolderSelectionNodeIdsStateAtom, next);
  },
);

export const selectedSubfolderNodeIdAtom = atom((get) => {
  const selected = get(subfolderSelectionNodeIdsStateAtom);
  return selected.size === 1 ? selected.values().next().value ?? null : null;
});

/**
 * The visible selected entity hashes in the loaded grid window.
 * For query-results selection, this means all loaded items except exclusions.
 */
export const selectedEntityHashesAtom = atom<
  Set<string>,
  [Set<string> | ((prev: Set<string>) => Set<string>)],
  void
>(
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
  (get, set, update) => {
    const prev = get(explicitSelectionHashesAtom);
    const next = typeof update === 'function' ? update(new Set(prev)) : new Set(update);
    set(selectionModeStateAtom, 'explicit');
    set(querySelectionExcludedHashesAtom, new Set<string>());
    set(subfolderSelectionNodeIdsStateAtom, new Set<string>());
    set(explicitSelectionHashesAtom, next);
  },
);

export const clearSelectionAtom = atom(null, (_get, set) => {
  set(selectionModeStateAtom, 'explicit');
  set(explicitSelectionHashesAtom, new Set<string>());
  set(querySelectionExcludedHashesAtom, new Set<string>());
  set(subfolderSelectionNodeIdsStateAtom, new Set<string>());
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
  set(subfolderSelectionNodeIdsStateAtom, new Set<string>());
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
  const selected = get(explicitSelectionHashesAtom);
  if (selected.size === 1) return selected.values().next().value as string;
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

export const selectionFingerprintAtom = atom((get) => {
  const target = get(selectionTargetAtom);
  const subfolderNodeId = get(selectedSubfolderNodeIdAtom);
  if (subfolderNodeId) return `subfolder:${subfolderNodeId}`;
  if (!target) return 'none';
  if (target.kind === 'query_results') {
    return JSON.stringify({
      kind: target.kind,
      query: target.query,
      excluded: [...get(querySelectionExcludedHashesAtom)].sort(),
    });
  }
  return JSON.stringify({
    kind: target.kind,
    hashes: [...get(explicitSelectionHashesAtom)].sort(),
  });
});
