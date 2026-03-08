import type { MasonryImageItem } from '../shared';
import type {
  GridViewMode,
  GridEmptyContext,
} from './gridRuntimeState';
import type { VirtualAllSelectionState } from './gridUiState';

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

// Viewer
type SetPopHash = { type: 'SET_POP_HASH'; hash: string | null };

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
  | SetPopHash
  | SetBoxActive
  | SetDragOver;
