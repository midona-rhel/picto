/**
 * Selection state — explicit item selection, virtual query-results selection,
 * and separate subfolder-tile selection for the header strip.
 *
 * Item actions always operate on canonical ItemTarget values.
 * Subfolder tile selection is scope-only UI state and never becomes an ItemTarget.
 */

import { atom } from 'jotai';
import type { ItemTarget } from '../shared/types/generated/application/ItemTarget';
import { currentGridQueryAtom, gridItemsAtom, gridTotalCountAtom } from './grid';

export type SelectionMode = 'explicit' | 'query_results';

const selectionModeStateAtom = atom<SelectionMode>('explicit');
const explicitSelectionItemIdsAtom = atom<Set<number>>(new Set<number>());
const querySelectionExcludedItemIdsAtom = atom<Set<number>>(new Set<number>());
const subfolderSelectionNodeIdsStateAtom = atom<Set<string>>(new Set<string>());

export const selectionModeAtom = atom((get) => get(selectionModeStateAtom));
export const querySelectionActiveAtom = atom((get) => get(selectionModeStateAtom) === 'query_results');

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
    set(explicitSelectionItemIdsAtom, new Set<number>());
    set(querySelectionExcludedItemIdsAtom, new Set<number>());
    set(subfolderSelectionNodeIdsStateAtom, next);
  },
);

export const selectedSubfolderNodeIdAtom = atom((get) => {
  const selected = get(subfolderSelectionNodeIdsStateAtom);
  return selected.size === 1 ? selected.values().next().value ?? null : null;
});

/**
 * The visible selected item IDs in the loaded grid window.
 * For query-results selection, this means all loaded items except exclusions.
 */
export const selectedItemIdsAtom = atom<
  Set<number>,
  [Set<number> | ((prev: Set<number>) => Set<number>)],
  void
>(
  (get) => {
    if (get(selectionModeStateAtom) === 'query_results') {
      const excluded = get(querySelectionExcludedItemIdsAtom);
      const selected = new Set<number>();
      for (const item of get(gridItemsAtom)) {
        if (!excluded.has(item.item_id)) {
          selected.add(item.item_id);
        }
      }
      return selected;
    }
    return get(explicitSelectionItemIdsAtom);
  },
  (get, set, update) => {
    const prev = get(explicitSelectionItemIdsAtom);
    const next = typeof update === 'function' ? update(new Set(prev)) : new Set(update);
    set(selectionModeStateAtom, 'explicit');
    set(querySelectionExcludedItemIdsAtom, new Set<number>());
    set(subfolderSelectionNodeIdsStateAtom, new Set<string>());
    set(explicitSelectionItemIdsAtom, next);
  },
);

export const clearSelectionAtom = atom(null, (_get, set) => {
  set(selectionModeStateAtom, 'explicit');
  set(explicitSelectionItemIdsAtom, new Set<number>());
  set(querySelectionExcludedItemIdsAtom, new Set<number>());
  set(subfolderSelectionNodeIdsStateAtom, new Set<string>());
});

export const selectAllResultsAtom = atom(null, (get, set) => {
  const totalCount = get(gridTotalCountAtom) ?? get(gridItemsAtom).length;
  if (totalCount <= 0) {
    set(clearSelectionAtom);
    return;
  }
  set(selectionModeStateAtom, 'query_results');
  set(explicitSelectionItemIdsAtom, new Set<number>());
  set(querySelectionExcludedItemIdsAtom, new Set<number>());
  set(subfolderSelectionNodeIdsStateAtom, new Set<string>());
});

export const toggleQuerySelectionItemIdAtom = atom(null, (get, set, itemId: number) => {
  if (get(selectionModeStateAtom) !== 'query_results') {
    return;
  }
  const excluded = new Set(get(querySelectionExcludedItemIdsAtom));
  if (excluded.has(itemId)) {
    excluded.delete(itemId);
  } else {
    excluded.add(itemId);
  }
  const totalCount = get(gridTotalCountAtom) ?? get(gridItemsAtom).length;
  if (excluded.size >= totalCount) {
    set(clearSelectionAtom);
    return;
  }
  set(querySelectionExcludedItemIdsAtom, excluded);
});

export const selectionCountAtom = atom((get) => {
  if (get(selectionModeStateAtom) === 'query_results') {
    const totalCount = get(gridTotalCountAtom) ?? get(gridItemsAtom).length;
    const count = totalCount - get(querySelectionExcludedItemIdsAtom).size;
    return count > 0 ? count : 0;
  }
  return get(explicitSelectionItemIdsAtom).size;
});

/** Single selected item ID only exists for explicit single selection. */
export const selectedItemIdAtom = atom((get) => {
  if (get(selectionModeStateAtom) !== 'explicit') return null;
  const selected = get(explicitSelectionItemIdsAtom);
  if (selected.size === 1) return selected.values().next().value as number;
  return null;
});

export const selectionTargetAtom = atom<ItemTarget | null>((get) => {
  const count = get(selectionCountAtom);
  if (count <= 0) return null;
  if (get(selectionModeStateAtom) === 'query_results') {
    return {
      kind: 'query',
      query: get(currentGridQueryAtom),
      excluded_item_ids: Array.from(get(querySelectionExcludedItemIdsAtom)),
    };
  }
  return {
    kind: 'explicit',
    item_ids: Array.from(get(explicitSelectionItemIdsAtom)),
  };
});

export const selectionFingerprintAtom = atom((get) => {
  const target = get(selectionTargetAtom);
  const subfolderNodeId = get(selectedSubfolderNodeIdAtom);
  if (subfolderNodeId) return `subfolder:${subfolderNodeId}`;
  if (!target) return 'none';
  if (target.kind === 'query') {
    return JSON.stringify({
      kind: target.kind,
      query: target.query,
      excluded: [...get(querySelectionExcludedItemIdsAtom)].sort((a, b) => a - b),
    });
  }
  return JSON.stringify({
    kind: target.kind,
    itemIds: [...get(explicitSelectionItemIdsAtom)].sort((a, b) => a - b),
  });
});
