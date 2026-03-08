import type { MasonryImageItem } from '../shared';
import type { GridEmptyContext, GridRuntimeInitProps, GridViewMode } from './gridRuntimeState';

export interface GridDataState {
  images: MasonryImageItem[];
  responseTotalCount: number | null;
  hasMore: boolean;
  defaultGridCursor: string | null;
  error: string | null;
  displayViewMode: GridViewMode;
  displayTargetSize: number;
  displayFolderId: number | null;
  displaySearchTags: string[] | undefined;
  displayEmptyContext: GridEmptyContext;
}

export function createInitialGridDataState(props: GridRuntimeInitProps): GridDataState {
  return {
    images: [],
    responseTotalCount: null,
    hasMore: true,
    defaultGridCursor: null,
    error: null,
    displayViewMode: props.viewMode,
    displayTargetSize: props.targetSize,
    displayFolderId: props.folderId,
    displaySearchTags: props.searchTags,
    displayEmptyContext: props.emptyContext,
  };
}
