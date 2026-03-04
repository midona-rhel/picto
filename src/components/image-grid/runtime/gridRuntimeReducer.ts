import type { MasonryImageItem } from '../shared';
import type {
  GridRuntimeState,
  GridViewMode,
  GridEmptyContext,
  VirtualAllSelectionState,
} from './gridRuntimeState';
import {
  createSession,
  navigateSession,
  rebaseSession,
  clampSession,
} from './gridViewerSession';

// ---------------------------------------------------------------------------
// Action types
// ---------------------------------------------------------------------------

// Dataset
type SetImages = { type: 'SET_IMAGES'; images: MasonryImageItem[] };
type AppendImages = { type: 'APPEND_IMAGES'; images: MasonryImageItem[]; maxItems: number };
type FilterImages = { type: 'FILTER_IMAGES'; predicate: (img: MasonryImageItem) => boolean };
type SetCursor = { type: 'SET_CURSOR'; cursor: string | null; hasMore: boolean };
type SetResponseTotalCount = { type: 'SET_RESPONSE_TOTAL_COUNT'; count: number | null };
type SetHasMore = { type: 'SET_HAS_MORE'; hasMore: boolean };
type SetError = { type: 'SET_ERROR'; error: string | null };
type ClearDataset = { type: 'CLEAR_DATASET' };

// Selection
type SelectHashes = { type: 'SELECT_HASHES'; hashes: Set<string> };
type ToggleHash = { type: 'TOGGLE_HASH'; hash: string };
type AddHashes = { type: 'ADD_HASHES'; hashes: string[] };
type RemoveHashes = { type: 'REMOVE_HASHES'; hashes: Set<string> };
type ClearSelection = { type: 'CLEAR_SELECTION' };
type SetLastClicked = { type: 'SET_LAST_CLICKED'; hash: string | null };
type ActivateVirtualSelectAll = {
  type: 'ACTIVATE_VIRTUAL_SELECT_ALL';
  baseSpec: VirtualAllSelectionState['baseSpec'];
};
type DeactivateVirtualSelectAll = { type: 'DEACTIVATE_VIRTUAL_SELECT_ALL' };
type ToggleVirtualExclusion = { type: 'TOGGLE_VIRTUAL_EXCLUSION'; hash: string };
type SetVirtualAllCount = { type: 'SET_VIRTUAL_ALL_COUNT'; count: number | null };
type SetSelectedSubfolder = { type: 'SET_SELECTED_SUBFOLDER'; id: number | null };

// Display
type CommitGeometry = {
  type: 'COMMIT_GEOMETRY';
  viewMode: GridViewMode;
  targetSize: number;
  folderId: number | null;
  searchTags: string[] | undefined;
  emptyContext: GridEmptyContext;
};

// Modal / Viewer
type OpenDetail = { type: 'OPEN_DETAIL'; hash: string };
type CloseDetail = { type: 'CLOSE_DETAIL' };
type OpenQuickLook = { type: 'OPEN_QUICK_LOOK'; hash: string };
type CloseQuickLook = { type: 'CLOSE_QUICK_LOOK' };
type SetPopHash = { type: 'SET_POP_HASH'; hash: string | null };
type ViewerNavigate = { type: 'VIEWER_NAVIGATE'; delta: number };


// Transition
type BeginFadeOut = { type: 'BEGIN_FADE_OUT' };
type CommitTransition = {
  type: 'COMMIT_TRANSITION';
  payload?: {
    images: MasonryImageItem[];
    nextCursor: string | null;
    hasMore: boolean;
  } | null;
  geometry?: {
    viewMode: GridViewMode;
    targetSize: number;
    folderId: number | null;
    searchTags: string[] | undefined;
    emptyContext: GridEmptyContext;
  };
  clearIfNoPayload?: boolean;
};
type AbortTransition = { type: 'ABORT_TRANSITION' };
type EndFade = { type: 'END_FADE' };

// Misc
type SetBoxActive = { type: 'SET_BOX_ACTIVE'; active: boolean };
type SetDragOver = { type: 'SET_DRAG_OVER'; over: boolean };

export type GridRuntimeAction =
  | SetImages
  | AppendImages
  | FilterImages
  | SetCursor
  | SetResponseTotalCount
  | SetHasMore
  | SetError
  | ClearDataset
  | SelectHashes
  | ToggleHash
  | AddHashes
  | RemoveHashes
  | ClearSelection
  | SetLastClicked
  | ActivateVirtualSelectAll
  | DeactivateVirtualSelectAll
  | ToggleVirtualExclusion
  | SetVirtualAllCount
  | SetSelectedSubfolder
  | CommitGeometry
  | OpenDetail
  | CloseDetail
  | OpenQuickLook
  | CloseQuickLook
  | SetPopHash
  | ViewerNavigate
  | BeginFadeOut
  | CommitTransition
  | AbortTransition
  | EndFade
  | SetBoxActive
  | SetDragOver;

// ---------------------------------------------------------------------------
// Reducer
// ---------------------------------------------------------------------------

export function gridRuntimeReducer(
  state: GridRuntimeState,
  action: GridRuntimeAction,
): GridRuntimeState {
  // Front-model mutation guard: during fade-out the visible grid is frozen.
  // These actions are silently dropped to prevent layout leaks.
  const frozen = state.transitionStage === 'fading_out';

  switch (action.type) {
    // ─── Dataset ─────────────────────────────────────────────────
    case 'SET_IMAGES': {
      if (frozen) return state;
      const next: GridRuntimeState = { ...state, images: action.images };
      if (state.viewerSession) {
        next.viewerSession = rebaseSession(state.viewerSession, action.images)
          ?? clampSession(state.viewerSession, action.images);
      }
      return next;
    }

    case 'APPEND_IMAGES': {
      if (frozen) return state;
      const merged = [...state.images, ...action.images];
      const next: GridRuntimeState = { ...state, images: merged };
      if (merged.length >= action.maxItems) next.hasMore = false;
      // Session rebase not needed for append — index stays valid, list only grew
      return next;
    }

    case 'FILTER_IMAGES': {
      if (frozen) return state;
      const filtered = state.images.filter(action.predicate);
      const next: GridRuntimeState = { ...state, images: filtered };
      if (state.viewerSession) {
        const rebased = rebaseSession(state.viewerSession, filtered);
        next.viewerSession = rebased ?? clampSession(state.viewerSession, filtered);
      }
      return next;
    }

    case 'SET_CURSOR':
      return { ...state, defaultGridCursor: action.cursor, hasMore: action.hasMore };

    case 'SET_RESPONSE_TOTAL_COUNT':
      return { ...state, responseTotalCount: action.count };

    case 'SET_HAS_MORE':
      return { ...state, hasMore: action.hasMore };

    case 'SET_ERROR':
      return { ...state, error: action.error };

    case 'CLEAR_DATASET':
      if (frozen) return state;
      return {
        ...state,
        images: [],
        defaultGridCursor: null,
        hasMore: false,
        responseTotalCount: null,
      };

    // ─── Selection ───────────────────────────────────────────────
    case 'SELECT_HASHES': {
      const nextState: GridRuntimeState = { ...state, selectedHashes: action.hashes };
      // Clear subfolder selection when grid images are selected (mirrors setSelectedHashes wrapper)
      if (action.hashes.size > 0) {
        nextState.selectedSubfolderId = null;
      }
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
      for (const h of action.hashes) next.add(h);
      return {
        ...state,
        selectedHashes: next,
        selectedSubfolderId: next.size > 0 ? null : state.selectedSubfolderId,
      };
    }

    case 'REMOVE_HASHES': {
      let changed = false;
      const next = new Set(state.selectedHashes);
      for (const h of action.hashes) {
        if (next.delete(h)) changed = true;
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

    // ─── Display ─────────────────────────────────────────────────
    case 'COMMIT_GEOMETRY':
      if (frozen) return state;
      return {
        ...state,
        displayViewMode: action.viewMode,
        displayTargetSize: action.targetSize,
        displayFolderId: action.folderId,
        displaySearchTags: action.searchTags,
        displayEmptyContext: action.emptyContext,
      };

    // ─── Modal / Viewer ─────────────────────────────────────────
    case 'OPEN_DETAIL':
      return { ...state, detailHash: action.hash, viewerSession: createSession(state.images, action.hash) };

    case 'CLOSE_DETAIL':
      return { ...state, detailHash: null, viewerSession: null };

    case 'OPEN_QUICK_LOOK':
      return { ...state, quickLookHash: action.hash, viewerSession: createSession(state.images, action.hash) };

    case 'CLOSE_QUICK_LOOK':
      return { ...state, quickLookHash: null, viewerSession: null };

    case 'SET_POP_HASH':
      return { ...state, popHash: action.hash };

    case 'VIEWER_NAVIGATE': {
      if (!state.viewerSession) return state;
      const nextSession = navigateSession(state.viewerSession, state.images, action.delta);
      if (nextSession === state.viewerSession) return state;
      return { ...state, viewerSession: nextSession };
    }

    // ─── Transition ──────────────────────────────────────────────
    case 'BEGIN_FADE_OUT':
      return { ...state, transitionStage: 'fading_out' };

    case 'COMMIT_TRANSITION': {
      let next: GridRuntimeState = {
        ...state,
        transitionStage: 'fading_in',
      };
      if (action.payload) {
        next.images = action.payload.images;
        next.defaultGridCursor = action.payload.nextCursor;
        next.hasMore = action.payload.hasMore;
      } else if (action.clearIfNoPayload) {
        next.images = [];
        next.defaultGridCursor = null;
        next.hasMore = false;
        next.responseTotalCount = null;
      }
      if (action.geometry) {
        next.displayViewMode = action.geometry.viewMode;
        next.displayTargetSize = action.geometry.targetSize;
        next.displayFolderId = action.geometry.folderId;
        next.displaySearchTags = action.geometry.searchTags;
        next.displayEmptyContext = action.geometry.emptyContext;
      }
      return next;
    }

    case 'ABORT_TRANSITION':
      return { ...state, transitionStage: 'idle' };

    case 'END_FADE':
      return { ...state, transitionStage: 'idle' };

    // ─── Misc ────────────────────────────────────────────────────
    case 'SET_BOX_ACTIVE':
      return { ...state, boxActive: action.active };

    case 'SET_DRAG_OVER':
      return { ...state, isDragOver: action.over };
  }
}
