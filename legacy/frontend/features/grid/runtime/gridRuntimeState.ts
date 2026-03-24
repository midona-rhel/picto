import type { GridDataState } from './gridDataState';
import type { GridUiState } from './gridUiState';

// ---------------------------------------------------------------------------
// Shared types (moved here from ImageGrid.tsx)
// ---------------------------------------------------------------------------

export type GridViewMode = 'waterfall' | 'justified' | 'grid';
export type GridEmptyContext = 'inbox' | 'uncategorized' | 'untagged' | 'folder' | 'smart-folder' | 'default';

// ---------------------------------------------------------------------------
// Grid Runtime State
// ---------------------------------------------------------------------------

export type GridRuntimeState = GridDataState & GridUiState;

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
