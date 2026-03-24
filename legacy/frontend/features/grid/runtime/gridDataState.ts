import type { MasonryItem } from '../shared';
import type { GridEmptyContext, GridRuntimeInitProps, GridViewMode } from './gridRuntimeState';
import type { TransitionStage } from './gridTransitionPipeline';

export interface GridDataState {
  images: MasonryItem[];
  responseTotalCount: number | null;
  hasMore: boolean;
  defaultGridCursor: string | null;
  error: string | null;
  transitionStage: TransitionStage;
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
    transitionStage: 'idle',
    displayViewMode: props.viewMode,
    displayTargetSize: props.targetSize,
    displayFolderId: props.folderId,
    displaySearchTags: props.searchTags,
    displayEmptyContext: props.emptyContext,
  };
}
