import { atom } from 'jotai';
import type { ItemTarget } from '../shared/types/generated/application/ItemTarget';
import { currentGridQueryAtom, gridItemsAtom, gridTotalCountAtom } from './grid';

export type SelectionMode = 'explicit' | 'query_results';

export interface GridSelection {
  mode: SelectionMode;
  itemIds: Set<number>;
  excludedItemIds: Set<number>;
  folderNodeIds: Set<string>;
  anchor: { kind: 'item'; id: number } | { kind: 'folder'; id: string } | null;
}

export type GridSelectionAction =
  | { type: 'clear' }
  | { type: 'replace_items'; itemIds: Set<number>; anchor?: number | null }
  | { type: 'toggle_item'; itemId: number }
  | { type: 'range_items'; itemIds: Set<number> }
  | { type: 'replace_folders'; ids: Set<string>; anchor?: string | null }
  | { type: 'toggle_folder'; id: string }
  | { type: 'marquee'; itemIds: Set<number>; folderNodeIds: Set<string>; additive: boolean }
  | { type: 'select_all'; totalCount: number }
  | { type: 'toggle_query_item'; itemId: number; totalCount: number }
  | { type: 'set_anchor'; anchor: GridSelection['anchor'] };

export const emptyGridSelection = (): GridSelection => ({
  mode: 'explicit',
  itemIds: new Set<number>(),
  excludedItemIds: new Set<number>(),
  folderNodeIds: new Set<string>(),
  anchor: null,
});

export function reduceGridSelection(state: GridSelection, action: GridSelectionAction): GridSelection {
  switch (action.type) {
    case 'clear':
      return emptyGridSelection();
    case 'replace_items':
      return {
        ...emptyGridSelection(),
        itemIds: new Set(action.itemIds),
        anchor: action.anchor == null ? null : { kind: 'item', id: action.anchor },
      };
    case 'toggle_item': {
      if (state.mode === 'query_results') {
        const excludedItemIds = new Set(state.excludedItemIds);
        if (excludedItemIds.has(action.itemId)) excludedItemIds.delete(action.itemId);
        else excludedItemIds.add(action.itemId);
        return { ...state, excludedItemIds, anchor: { kind: 'item', id: action.itemId } };
      }
      const itemIds = state.folderNodeIds.size > 0 ? new Set<number>() : new Set(state.itemIds);
      if (itemIds.has(action.itemId)) itemIds.delete(action.itemId);
      else itemIds.add(action.itemId);
      return {
        ...state,
        itemIds,
        folderNodeIds: new Set(),
        anchor: { kind: 'item', id: action.itemId },
      };
    }
    case 'range_items':
      return {
        ...emptyGridSelection(),
        itemIds: new Set(action.itemIds),
        anchor: state.anchor?.kind === 'item' ? state.anchor : null,
      };
    case 'replace_folders':
      return { ...emptyGridSelection(), folderNodeIds: new Set(action.ids), anchor: action.anchor == null ? null : { kind: 'folder', id: action.anchor } };
    case 'toggle_folder': {
      const folderNodeIds = state.mode === 'query_results' || state.itemIds.size > 0
        ? new Set<string>()
        : new Set(state.folderNodeIds);
      if (folderNodeIds.has(action.id)) folderNodeIds.delete(action.id);
      else folderNodeIds.add(action.id);
      return {
        ...emptyGridSelection(),
        folderNodeIds,
        anchor: { kind: 'folder', id: action.id },
      };
    }
    case 'marquee': {
      const selectingFolders = action.folderNodeIds.size > 0;
      const canAddToFolders = action.additive && state.mode === 'explicit' && state.itemIds.size === 0;
      const canAddToItems = action.additive && state.mode === 'explicit' && state.folderNodeIds.size === 0;
      return {
        ...emptyGridSelection(),
        itemIds: selectingFolders
          ? new Set()
          : canAddToItems ? new Set([...state.itemIds, ...action.itemIds]) : new Set(action.itemIds),
        folderNodeIds: selectingFolders
          ? canAddToFolders
            ? new Set([...state.folderNodeIds, ...action.folderNodeIds])
            : new Set(action.folderNodeIds)
          : new Set(),
      };
    }
    case 'select_all':
      return action.totalCount > 0 ? { ...emptyGridSelection(), mode: 'query_results' } : emptyGridSelection();
    case 'toggle_query_item': {
      if (state.mode !== 'query_results') return state;
      const excludedItemIds = new Set(state.excludedItemIds);
      if (excludedItemIds.has(action.itemId)) excludedItemIds.delete(action.itemId);
      else excludedItemIds.add(action.itemId);
      return excludedItemIds.size >= action.totalCount
        ? emptyGridSelection()
        : { ...state, excludedItemIds, anchor: { kind: 'item', id: action.itemId } };
    }
    case 'set_anchor':
      return { ...state, anchor: action.anchor };
  }
}

export const gridSelectionAtom = atom<GridSelection>(emptyGridSelection());
export const gridSelectionActionAtom = atom(null, (get, set, action: GridSelectionAction) => {
  set(gridSelectionAtom, reduceGridSelection(get(gridSelectionAtom), action));
});

export const selectionModeAtom = atom((get) => get(gridSelectionAtom).mode);
export const querySelectionActiveAtom = atom((get) => get(selectionModeAtom) === 'query_results');

/** Loaded IDs are a viewport projection; query-wide selection remains canonical in the target. */
export const loadedSelectedItemIdsAtom = atom((get) => {
  const selection = get(gridSelectionAtom);
  if (selection.mode === 'explicit') return new Set(selection.itemIds);
  return new Set(
    get(gridItemsAtom)
      .map((item) => item.item_id)
      .filter((itemId) => !selection.excludedItemIds.has(itemId)),
  );
});

export const selectedItemIdsAtom = atom<
  Set<number>,
  [Set<number> | ((previous: Set<number>) => Set<number>)],
  void
>(
  (get) => get(loadedSelectedItemIdsAtom),
  (get, set, update) => {
    const previous = get(loadedSelectedItemIdsAtom);
    const next = typeof update === 'function' ? update(new Set(previous)) : update;
    set(gridSelectionAtom, { ...emptyGridSelection(), itemIds: new Set(next) });
  },
);

export const toggleQuerySelectionItemIdAtom = atom(null, (get, set, itemId: number) => {
  if (get(selectionModeAtom) !== 'query_results') return;
  set(gridSelectionActionAtom, {
    type: 'toggle_query_item',
    itemId,
    totalCount: get(gridTotalCountAtom) ?? get(gridItemsAtom).length,
  });
});

export const selectedSubfolderNodeIdsAtom = atom<
  Set<string>,
  [Set<string> | ((previous: Set<string>) => Set<string>)],
  void
>(
  (get) => get(gridSelectionAtom).folderNodeIds,
  (get, set, update) => {
    const previous = get(gridSelectionAtom).folderNodeIds;
    const next = typeof update === 'function' ? update(new Set(previous)) : update;
    set(gridSelectionAtom, { ...emptyGridSelection(), folderNodeIds: new Set(next) });
  },
);

export const selectedSubfolderNodeIdAtom = atom((get) => {
  const ids = get(selectedSubfolderNodeIdsAtom);
  return ids.size === 1 ? ids.values().next().value ?? null : null;
});

export const selectedItemIdAtom = atom((get) => {
  const selection = get(gridSelectionAtom);
  return selection.mode === 'explicit' && selection.itemIds.size === 1
    ? selection.itemIds.values().next().value ?? null
    : null;
});

export const selectionCountAtom = atom((get) => {
  const selection = get(gridSelectionAtom);
  if (selection.mode === 'query_results') {
    const totalCount = get(gridTotalCountAtom) ?? get(gridItemsAtom).length;
    return Math.max(0, totalCount - selection.excludedItemIds.size);
  }
  return selection.itemIds.size;
});

export const selectionTargetAtom = atom<ItemTarget | null>((get) => {
  if (get(selectionCountAtom) <= 0) return null;
  const selection = get(gridSelectionAtom);
  return selection.mode === 'query_results'
    ? { kind: 'query', query: get(currentGridQueryAtom), excluded_item_ids: [...selection.excludedItemIds] }
    : { kind: 'explicit', item_ids: [...selection.itemIds] };
});

export const selectionFingerprintAtom = atom((get) => {
  const selection = get(gridSelectionAtom);
  return JSON.stringify({ target: get(selectionTargetAtom), folders: [...selection.folderNodeIds].sort() });
});

export const clearSelectionAtom = atom(null, (_get, set) => set(gridSelectionActionAtom, { type: 'clear' }));
export const selectAllResultsAtom = atom(null, (get, set) => set(
  gridSelectionActionAtom,
  { type: 'select_all', totalCount: get(gridTotalCountAtom) ?? get(gridItemsAtom).length },
));
