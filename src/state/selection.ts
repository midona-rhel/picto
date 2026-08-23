import { atom } from 'jotai';
import type { EntityTarget } from '../shared/types/canonical';
import { currentGridQueryAtom, gridItemsAtom, gridTotalCountAtom } from './grid';

export type SelectionMode = 'explicit' | 'query_results';
export interface GridSelection {
  mode: SelectionMode;
  entityHashes: Set<string>;
  excludedEntityHashes: Set<string>;
  folderNodeIds: Set<string>;
  anchor: { kind: 'entity' | 'folder'; id: string } | null;
}
export type GridSelectionAction =
  | { type: 'clear' }
  | { type: 'replace_entities'; hashes: Set<string>; anchor?: string | null }
  | { type: 'toggle_entity'; hash: string }
  | { type: 'range_entities'; hashes: Set<string> }
  | { type: 'replace_folders'; ids: Set<string>; anchor?: string | null }
  | { type: 'toggle_folder'; id: string }
  | { type: 'marquee'; entityHashes: Set<string>; folderNodeIds: Set<string>; additive: boolean }
  | { type: 'select_all'; totalCount: number }
  | { type: 'toggle_query_entity'; hash: string; totalCount: number }
  | { type: 'set_anchor'; anchor: GridSelection['anchor'] };

export const emptyGridSelection = (): GridSelection => ({
  mode: 'explicit', entityHashes: new Set(), excludedEntityHashes: new Set(),
  folderNodeIds: new Set(), anchor: null,
});

export function reduceGridSelection(state: GridSelection, action: GridSelectionAction): GridSelection {
  switch (action.type) {
    case 'clear': return emptyGridSelection();
    case 'replace_entities': return { ...emptyGridSelection(), entityHashes: action.hashes, anchor: action.anchor ? { kind: 'entity', id: action.anchor } : null };
    case 'toggle_entity': {
      if (state.mode === 'query_results') {
        const excludedEntityHashes = new Set(state.excludedEntityHashes);
        excludedEntityHashes.has(action.hash) ? excludedEntityHashes.delete(action.hash) : excludedEntityHashes.add(action.hash);
        return { ...state, excludedEntityHashes, anchor: { kind: 'entity', id: action.hash } };
      }
      const entityHashes = new Set(state.entityHashes);
      entityHashes.has(action.hash) ? entityHashes.delete(action.hash) : entityHashes.add(action.hash);
      return { ...state, entityHashes, anchor: { kind: 'entity', id: action.hash } };
    }
    case 'range_entities': return { ...state, mode: 'explicit', entityHashes: action.hashes, excludedEntityHashes: new Set() };
    case 'replace_folders': return { ...emptyGridSelection(), folderNodeIds: action.ids, anchor: action.anchor ? { kind: 'folder', id: action.anchor } : null };
    case 'toggle_folder': {
      const folderNodeIds = new Set(state.folderNodeIds);
      folderNodeIds.has(action.id) ? folderNodeIds.delete(action.id) : folderNodeIds.add(action.id);
      return { ...state, folderNodeIds, anchor: { kind: 'folder', id: action.id } };
    }
    case 'marquee': return {
      ...state, mode: 'explicit', excludedEntityHashes: new Set(),
      entityHashes: action.additive ? new Set([...state.entityHashes, ...action.entityHashes]) : action.entityHashes,
      folderNodeIds: action.additive ? new Set([...state.folderNodeIds, ...action.folderNodeIds]) : action.folderNodeIds,
    };
    case 'select_all': return action.totalCount > 0 ? { ...emptyGridSelection(), mode: 'query_results' } : emptyGridSelection();
    case 'toggle_query_entity': {
      if (state.mode !== 'query_results') return state;
      const excludedEntityHashes = new Set(state.excludedEntityHashes);
      excludedEntityHashes.has(action.hash) ? excludedEntityHashes.delete(action.hash) : excludedEntityHashes.add(action.hash);
      return excludedEntityHashes.size >= action.totalCount ? emptyGridSelection() : { ...state, excludedEntityHashes };
    }
    case 'set_anchor': return { ...state, anchor: action.anchor };
  }
}

export const gridSelectionAtom = atom<GridSelection>(emptyGridSelection());
export const gridSelectionActionAtom = atom(null, (get, set, action: GridSelectionAction) => {
  set(gridSelectionAtom, reduceGridSelection(get(gridSelectionAtom), action));
});
export const loadedSelectedEntityHashesAtom = atom((get) => {
  const selection = get(gridSelectionAtom);
  if (selection.mode === 'explicit') return selection.entityHashes;
  return new Set(get(gridItemsAtom).map((item) => item.entity_hash).filter((hash) => !selection.excludedEntityHashes.has(hash)));
});
export const selectedFolderNodeIdAtom = atom((get) => {
  const ids = get(gridSelectionAtom).folderNodeIds;
  return ids.size === 1 ? ids.values().next().value ?? null : null;
});
export const selectionCountAtom = atom((get) => {
  const selection = get(gridSelectionAtom);
  return selection.mode === 'query_results'
    ? Math.max(0, (get(gridTotalCountAtom) ?? get(gridItemsAtom).length) - selection.excludedEntityHashes.size)
    : selection.entityHashes.size;
});
export const selectedEntityHashAtom = atom((get) => {
  const selection = get(gridSelectionAtom);
  return selection.mode === 'explicit' && selection.entityHashes.size === 1 ? selection.entityHashes.values().next().value ?? null : null;
});
export const selectionTargetAtom = atom<EntityTarget | null>((get) => {
  const selection = get(gridSelectionAtom);
  if (get(selectionCountAtom) === 0) return null;
  return selection.mode === 'query_results'
    ? { kind: 'query_results', query: get(currentGridQueryAtom), excluded_entity_hashes: [...selection.excludedEntityHashes] }
    : { kind: 'entity_hashes', entity_hashes: [...selection.entityHashes] };
});
export const selectionFingerprintAtom = atom((get) => {
  const selection = get(gridSelectionAtom);
  if (selection.folderNodeIds.size) return `folders:${[...selection.folderNodeIds].sort().join(',')}`;
  const target = get(selectionTargetAtom);
  return target ? JSON.stringify(target) : 'none';
});
export const clearSelectionAtom = atom(null, (_get, set) => set(gridSelectionActionAtom, { type: 'clear' }));
export const selectAllResultsAtom = atom(null, (get, set) => set(gridSelectionActionAtom, { type: 'select_all', totalCount: get(gridTotalCountAtom) ?? get(gridItemsAtom).length }));
