import type { GridRuntimeAction } from './gridRuntimeReducer';
import type { GridUiState } from './gridUiState';

function isUiAction(action: GridRuntimeAction): boolean {
  return action.type === 'SELECT_HASHES'
    || action.type === 'TOGGLE_HASH'
    || action.type === 'ADD_HASHES'
    || action.type === 'REMOVE_HASHES'
    || action.type === 'CLEAR_SELECTION'
    || action.type === 'SET_LAST_CLICKED'
    || action.type === 'ACTIVATE_VIRTUAL_SELECT_ALL'
    || action.type === 'DEACTIVATE_VIRTUAL_SELECT_ALL'
    || action.type === 'TOGGLE_VIRTUAL_EXCLUSION'
    || action.type === 'SET_VIRTUAL_ALL_COUNT'
    || action.type === 'SET_SELECTED_SUBFOLDER'
    || action.type === 'SET_POP_HASH'
    || action.type === 'SET_BOX_ACTIVE'
    || action.type === 'SET_DRAG_OVER';
}

export function gridUiReducer(state: GridUiState, action: GridRuntimeAction): GridUiState {
  if (!isUiAction(action)) return state;

  switch (action.type) {
    case 'SELECT_HASHES': {
      const nextState: GridUiState = { ...state, selectedHashes: action.hashes };
      if (action.hashes.size > 0) nextState.selectedSubfolderId = null;
      return nextState;
    }

    case 'TOGGLE_HASH': {
      const next = new Set(state.selectedHashes);
      if (next.has(action.hash)) next.delete(action.hash);
      else next.add(action.hash);
      return {
        ...state,
        selectedHashes: next,
        selectedSubfolderId: next.size > 0 ? null : state.selectedSubfolderId,
      };
    }

    case 'ADD_HASHES': {
      const next = new Set(state.selectedHashes);
      for (const hash of action.hashes) next.add(hash);
      return {
        ...state,
        selectedHashes: next,
        selectedSubfolderId: next.size > 0 ? null : state.selectedSubfolderId,
      };
    }

    case 'REMOVE_HASHES': {
      let changed = false;
      const next = new Set(state.selectedHashes);
      for (const hash of action.hashes) {
        if (next.delete(hash)) changed = true;
      }
      return changed ? { ...state, selectedHashes: next } : state;
    }

    case 'CLEAR_SELECTION':
      return {
        ...state,
        selectedHashes: new Set(),
        virtualAllSelection: null,
        virtualAllSelectedCount: null,
        lastClickedHash: null,
      };

    case 'SET_LAST_CLICKED':
      return { ...state, lastClickedHash: action.hash };

    case 'ACTIVATE_VIRTUAL_SELECT_ALL':
      return {
        ...state,
        virtualAllSelection: { baseSpec: action.baseSpec, excludedHashes: new Set() },
        selectedHashes: new Set(),
        selectedSubfolderId: null,
        lastClickedHash: null,
      };

    case 'DEACTIVATE_VIRTUAL_SELECT_ALL':
      return { ...state, virtualAllSelection: null };

    case 'TOGGLE_VIRTUAL_EXCLUSION': {
      if (!state.virtualAllSelection) return state;
      const nextExcluded = new Set(state.virtualAllSelection.excludedHashes);
      if (nextExcluded.has(action.hash)) nextExcluded.delete(action.hash);
      else nextExcluded.add(action.hash);
      return {
        ...state,
        virtualAllSelection: { ...state.virtualAllSelection, excludedHashes: nextExcluded },
      };
    }

    case 'SET_VIRTUAL_ALL_COUNT':
      return { ...state, virtualAllSelectedCount: action.count };

    case 'SET_SELECTED_SUBFOLDER':
      return { ...state, selectedSubfolderId: action.id };

    case 'SET_POP_HASH':
      return { ...state, popHash: action.hash };

    case 'SET_BOX_ACTIVE':
      return { ...state, boxActive: action.active };

    case 'SET_DRAG_OVER':
      return { ...state, isDragOver: action.over };
  }

  return state;
}
