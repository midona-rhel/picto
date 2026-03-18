import type { MasonryImageItem } from './shared';
import type { GridEmptyContext, GridViewMode } from './runtime';

export interface GridSurfaceModel {
  scopeKey: string;
  images: MasonryImageItem[];
  responseTotalCount: number | null;
  totalCount: number | null;
  hasMore: boolean;
  displayViewMode: GridViewMode;
  displayTargetSize: number;
  displayFolderId: number | null;
  displaySearchTags: string[] | undefined;
  displayEmptyContext: GridEmptyContext;
  selectedSubfolderId: number | null;
  showEmptyState: boolean;
}

export function buildGridSurfaceModel(args: {
  scopeKey: string;
  images: MasonryImageItem[];
  responseTotalCount: number | null;
  totalCount: number | null;
  hasMore: boolean;
  displayViewMode: GridViewMode;
  displayTargetSize: number;
  displayFolderId: number | null;
  displaySearchTags: string[] | undefined;
  displayEmptyContext: GridEmptyContext;
  selectedSubfolderId: number | null;
  showEmptyState: boolean;
}): GridSurfaceModel {
  const {
    scopeKey,
    images,
    responseTotalCount,
    totalCount,
    hasMore,
    displayViewMode,
    displayTargetSize,
    displayFolderId,
    displaySearchTags,
    displayEmptyContext,
    selectedSubfolderId,
    showEmptyState,
  } = args;

  return {
    scopeKey,
    images,
    responseTotalCount,
    totalCount,
    hasMore,
    displayViewMode,
    displayTargetSize,
    displayFolderId,
    displaySearchTags,
    displayEmptyContext,
    selectedSubfolderId,
    showEmptyState,
  };
}

export function equalGridSurfaceModel(a: GridSurfaceModel, b: GridSurfaceModel): boolean {
  return a.scopeKey === b.scopeKey
    && a.images === b.images
    && a.responseTotalCount === b.responseTotalCount
    && a.totalCount === b.totalCount
    && a.hasMore === b.hasMore
    && a.displayViewMode === b.displayViewMode
    && a.displayTargetSize === b.displayTargetSize
    && a.displayFolderId === b.displayFolderId
    && a.displaySearchTags === b.displaySearchTags
    && a.displayEmptyContext === b.displayEmptyContext
    && a.selectedSubfolderId === b.selectedSubfolderId
    && a.showEmptyState === b.showEmptyState;
}
