import type { MasonryImageItem } from '../shared';
import type { SelectionQuerySpec } from '../metadataPrefetch';
import type { TransitionStage } from './gridTransitionPipeline';
import type { ViewerSession } from './gridViewerSession';

// ---------------------------------------------------------------------------
// Shared types (moved here from ImageGrid.tsx)
// ---------------------------------------------------------------------------

export type GridViewMode = 'waterfall' | 'justified' | 'grid';
export type GridEmptyContext = 'inbox' | 'untagged' | 'folder' | 'smart-folder' | 'default';

export type VirtualAllSelectionState = {
  baseSpec: Omit<SelectionQuerySpec, 'excluded_hashes'>;
  excludedHashes: Set<string>;
};

// ---------------------------------------------------------------------------
// Grid Runtime State
// ---------------------------------------------------------------------------

export interface GridRuntimeState {
  // Dataset
  images: MasonryImageItem[];
  responseTotalCount: number | null;
  hasMore: boolean;
  defaultGridCursor: string | null;
  error: string | null;

  // Selection
  selectedHashes: Set<string>;
  virtualAllSelection: VirtualAllSelectionState | null;
  virtualAllSelectedCount: number | null;
  lastClickedHash: string | null;
  selectedSubfolderId: number | null;

  // Display (deferred geometry — applied at transition commit)
  displayViewMode: GridViewMode;
  displayTargetSize: number;
  displayFolderId: number | null;
  displaySearchTags: string[] | undefined;
  displayEmptyContext: GridEmptyContext;

  // Modal / Viewer
  detailHash: string | null;
  quickLookHash: string | null;
  popHash: string | null;
  viewerSession: ViewerSession | null;

  // Transition
  transitionStage: TransitionStage;

  // Misc
  boxActive: boolean;
  isDragOver: boolean;
}

// ---------------------------------------------------------------------------
// Initial state factory
// ---------------------------------------------------------------------------

export interface GridRuntimeInitProps {
  viewMode: GridViewMode;
  targetSize: number;
  folderId: number | null;
  searchTags: string[] | undefined;
  emptyContext: GridEmptyContext;
}

export function createInitialState(props: GridRuntimeInitProps): GridRuntimeState {
  return {
    // Dataset
    images: [],
    responseTotalCount: null,
    hasMore: true,
    defaultGridCursor: null,
    error: null,

    // Selection
    selectedHashes: new Set(),
    virtualAllSelection: null,
    virtualAllSelectedCount: null,
    lastClickedHash: null,
    selectedSubfolderId: null,

    // Display
    displayViewMode: props.viewMode,
    displayTargetSize: props.targetSize,
    displayFolderId: props.folderId,
    displaySearchTags: props.searchTags,
    displayEmptyContext: props.emptyContext,

    // Modal / Viewer
    detailHash: null,
    quickLookHash: null,
    popHash: null,
    viewerSession: null,

    // Transition — start invisible; first COMMIT_TRANSITION fades in
    transitionStage: 'fading_out',

    // Misc
    boxActive: false,
    isDragOver: false,
  };
}
