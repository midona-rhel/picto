import { atom } from 'jotai';
import type { EntityTarget, EntityViewQuery } from '../shared/types/canonical';
import { currentGridQueryAtom, gridItemsAtom, gridSessionAtom, gridTotalCountAtom } from './grid';
import {
  addSpan,
  rangeCount,
  reconcileRange,
  spansContain,
  toggleInRange,
  type RangeEnd,
  type RangeSelection,
  type RangeSpan,
  type RootPositions,
} from './rangeSelection';

export type { RangeEnd, RangeSpan } from './rangeSelection';

export type SelectionMode = 'explicit' | 'query_results';

export interface GridSelection {
  mode: SelectionMode;
  itemIds: Set<number>;
  excludedItemIds: Set<number>;
  query?: EntityViewQuery | null;
  queryTotalCount?: number | null;
  folderNodeIds: Set<string>;
  /** `index` is the anchor's position in the whole view, when known. */
  anchor: { kind: 'item'; id: number; index?: number } | { kind: 'folder'; id: string } | null;
  /**
   * Shift-click spans that reach beyond the loaded window, with roots added
   * outside them (see rangeSelection.ts). `excludedItemIds` holds the roots
   * toggled off inside them. The backend resolves it.
   */
  range?: RangeSelection | null;
}

export type GridSelectionAction =
  | { type: 'clear' }
  | { type: 'replace_items'; itemIds: Set<number>; anchor?: number | null; anchorIndex?: number }
  | { type: 'toggle_item'; itemId: number; index?: number }
  | { type: 'range_items'; itemIds: Set<number> }
  | { type: 'range_query'; query: EntityViewQuery; anchor: RangeEnd; focus: RangeEnd }
  /** Cmd/Ctrl+Shift-click: add a span to the current selection. */
  | { type: 'range_add'; query: EntityViewQuery; span: RangeSpan; positions: RootPositions }
  /** Backend positions after the view changed. */
  | { type: 'range_reconcile'; positions: RootPositions }
  | { type: 'replace_folders'; ids: Set<string>; anchor?: string | null }
  | { type: 'toggle_folder'; id: string }
  | { type: 'marquee'; itemIds: Set<number>; folderNodeIds: Set<string>; additive: boolean }
  | { type: 'select_all'; totalCount: number; query?: EntityViewQuery }
  | { type: 'toggle_query_item'; itemId: number; totalCount: number; index?: number }
  | { type: 'set_anchor'; anchor: GridSelection['anchor'] };

export const emptyGridSelection = (): GridSelection => ({
  mode: 'explicit',
  itemIds: new Set<number>(),
  excludedItemIds: new Set<number>(),
  query: null,
  queryTotalCount: null,
  folderNodeIds: new Set<string>(),
  anchor: null,
  range: null,
});

function itemAnchor(id: number, index?: number): GridSelection['anchor'] {
  return index == null ? { kind: 'item', id } : { kind: 'item', id, index };
}

export function reduceGridSelection(state: GridSelection, action: GridSelectionAction): GridSelection {
  switch (action.type) {
    case 'clear':
      return emptyGridSelection();
    case 'replace_items':
      return {
        ...emptyGridSelection(),
        itemIds: new Set(action.itemIds),
        anchor: action.anchor == null ? null : itemAnchor(action.anchor, action.anchorIndex),
      };
    case 'toggle_item': {
      if (state.mode === 'query_results') {
        const excludedItemIds = new Set(state.excludedItemIds);
        if (excludedItemIds.has(action.itemId)) excludedItemIds.delete(action.itemId);
        else excludedItemIds.add(action.itemId);
        return { ...state, excludedItemIds, anchor: itemAnchor(action.itemId, action.index) };
      }
      const itemIds = state.folderNodeIds.size > 0 ? new Set<number>() : new Set(state.itemIds);
      if (itemIds.has(action.itemId)) itemIds.delete(action.itemId);
      else itemIds.add(action.itemId);
      return {
        ...state,
        itemIds,
        folderNodeIds: new Set(),
        anchor: itemAnchor(action.itemId, action.index),
      };
    }
    case 'range_items':
      return {
        ...emptyGridSelection(),
        itemIds: new Set(action.itemIds),
        anchor: state.anchor?.kind === 'item' ? state.anchor : null,
      };
    case 'range_query':
      return withRange(
        emptyGridSelection(),
        action.query,
        { spans: [{ anchor: action.anchor, focus: action.focus }], includedItemIds: new Set() },
        new Set(),
        itemAnchor(action.anchor.id, action.anchor.index),
      );
    case 'range_add': {
      const anchor = itemAnchor(action.span.anchor.id, action.span.anchor.index);
      if (state.mode === 'query_results' && !state.range) {
        // Select-all plus a span: only exclusions the span covers come back.
        const covered = addSpan([], new Set(), state.excludedItemIds, action.span, action.positions);
        return { ...state, excludedItemIds: covered.excluded, anchor };
      }
      const previous = state.range
        ? state.range.includedItemIds
        : state.folderNodeIds.size > 0 ? new Set<number>() : state.itemIds;
      const next = addSpan(state.range?.spans ?? [], previous, state.excludedItemIds, action.span, action.positions);
      return withRange(emptyGridSelection(), action.query, next.range, next.excluded, anchor);
    }
    case 'range_reconcile': {
      if (!state.range || !state.query) return state;
      const next = reconcileRange(state.range, state.excludedItemIds, action.positions);
      if (next.range.spans.length === 0 && next.range.includedItemIds.size === 0) return emptyGridSelection();
      return withRange(state, state.query, next.range, next.excluded, state.anchor);
    }
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
      return action.totalCount > 0
        ? {
            ...emptyGridSelection(),
            mode: 'query_results',
            query: action.query ?? null,
            queryTotalCount: action.totalCount,
          }
        : emptyGridSelection();
    case 'toggle_query_item': {
      if (state.mode !== 'query_results') return state;
      if (state.range && state.query && action.index != null) {
        const next = toggleInRange(state.range, state.excludedItemIds, action.itemId, action.index);
        return rangeCount(next.range, next.excluded) <= 0
          ? emptyGridSelection()
          : withRange(state, state.query, next.range, next.excluded, state.anchor);
      }
      const excludedItemIds = new Set(state.excludedItemIds);
      if (excludedItemIds.has(action.itemId)) excludedItemIds.delete(action.itemId);
      else excludedItemIds.add(action.itemId);
      return excludedItemIds.size >= (state.queryTotalCount ?? action.totalCount)
        ? emptyGridSelection()
        : { ...state, excludedItemIds, anchor: state.range ? state.anchor : itemAnchor(action.itemId, action.index) };
    }
    case 'set_anchor':
      return { ...state, anchor: action.anchor };
  }
}

function withRange(
  base: GridSelection,
  query: EntityViewQuery,
  range: RangeSelection,
  excluded: Set<number>,
  anchor: GridSelection['anchor'],
): GridSelection {
  return {
    ...base,
    mode: 'query_results',
    itemIds: new Set(),
    folderNodeIds: new Set(),
    query,
    range,
    excludedItemIds: excluded,
    queryTotalCount: rangeCount(range, excluded),
    anchor,
  };
}

export const gridSelectionAtom = atom<GridSelection>(emptyGridSelection());
export const gridSelectionActionAtom = atom(null, (get, set, action: GridSelectionAction) => {
  set(gridSelectionAtom, reduceGridSelection(get(gridSelectionAtom), action));
});

export const selectionModeAtom = atom((get) => get(gridSelectionAtom).mode);

/** Loaded IDs are a viewport projection; query-wide selection remains canonical in the target. */
export const loadedSelectedItemIdsAtom = atom((get) => {
  const selection = get(gridSelectionAtom);
  if (selection.mode === 'explicit') return new Set(selection.itemIds);
  const items = get(gridItemsAtom);
  const range = selection.range;
  if (range) {
    // Loaded tiles inside a span (unless toggled off), plus added roots.
    const start = get(gridSessionAtom).windowStart;
    const selected = new Set<number>();
    items.forEach((item, offset) => {
      const id = item.root_id;
      if (range.includedItemIds.has(id)) selected.add(id);
      else if (spansContain(range.spans, start + offset) && !selection.excludedItemIds.has(id)) selected.add(id);
    });
    return selected;
  }
  return new Set(
    items
      .map((item) => item.root_id)
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
  if (selection.range) return rangeCount(selection.range, selection.excludedItemIds);
  if (selection.mode === 'query_results') {
    const totalCount = selection.queryTotalCount
      ?? get(gridTotalCountAtom)
      ?? get(gridItemsAtom).length;
    return Math.max(0, totalCount - selection.excludedItemIds.size);
  }
  return selection.itemIds.size;
});

export const selectionTargetAtom = atom<EntityTarget | null>((get) => {
  if (get(selectionCountAtom) <= 0) return null;
  const selection = get(gridSelectionAtom);
  if (selection.mode === 'query_results') {
    if (selection.query == null) return null;
    if (selection.range) {
      return {
        kind: 'range',
        query: selection.query,
        spans: selection.range.spans.map((span) => ({
          anchor_root_id: span.anchor.id,
          focus_root_id: span.focus.id,
          anchor_index: span.anchor.index,
          focus_index: span.focus.index,
        })),
        included_root_ids: [...selection.range.includedItemIds],
        excluded_root_ids: [...selection.excludedItemIds],
      };
    }
    return {
      kind: 'query',
      query: selection.query,
      excluded_root_ids: [...selection.excludedItemIds],
    };
  }
  return { kind: 'explicit', root_ids: [...selection.itemIds] };
});

export const selectionFingerprintAtom = atom((get) => {
  const selection = get(gridSelectionAtom);
  return JSON.stringify({ target: get(selectionTargetAtom), folders: [...selection.folderNodeIds].sort() });
});

export const clearSelectionAtom = atom(null, (_get, set) => set(gridSelectionActionAtom, { type: 'clear' }));
export const selectAllResultsAtom = atom(null, (get, set) => set(
  gridSelectionActionAtom,
  {
    type: 'select_all',
    totalCount: get(gridTotalCountAtom) ?? get(gridItemsAtom).length,
    query: get(currentGridQueryAtom),
  },
));
