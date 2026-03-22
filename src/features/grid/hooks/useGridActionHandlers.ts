import { useCallback } from 'react';
import type { MutableRefObject } from 'react';
import type { LayoutItem } from '../layoutMath';
import type { MasonryItem } from '../shared';
import type { ViewerHostController } from '../../../features/viewer/hooks/useViewerHost';
import type { GridRuntimeAction, GridRuntimeState } from '../runtime';
import { useGridStateActions } from './useGridStateActions';
import { useGridItemActions } from './useGridItemActions';
import { useGridImageClick } from './useGridImageClick';
import { useGridReorder } from './useGridReorder';

export function useGridActionHandlers(args: {
  state: GridRuntimeState;
  stateRef: MutableRefObject<GridRuntimeState>;
  dispatch: React.Dispatch<GridRuntimeAction>;
  imagesRef: MutableRefObject<MasonryItem[]>;
  lastClickedHashRef: MutableRefObject<string | null>;
  canvasLayoutRef: MutableRefObject<LayoutItem[]>;
  viewer: ViewerHostController;
  selectedScopeCount: number | null;
  statusFilter?: string | null;
  folderId?: number | null;
  collectionEntityId?: number | null;
  requestReplace: () => Promise<void>;
  requestAppend: () => Promise<void>;
  displayFolderId: number | null;
  dismissHoverPreviewRef: MutableRefObject<() => void>;
  dismissVideoScrubRef: MutableRefObject<() => void>;
}) {
  const {
    state,
    stateRef,
    dispatch,
    imagesRef,
    lastClickedHashRef,
    canvasLayoutRef,
    viewer,
    selectedScopeCount,
    statusFilter,
    folderId,
    collectionEntityId,
    requestReplace,
    requestAppend,
    displayFolderId,
    dismissHoverPreviewRef,
    dismissVideoScrubRef,
  } = args;

  const singleSelectedHash = !state.virtualAllSelection && state.selectedHashes.size === 1
    ? [...state.selectedHashes][0]
    : null;

  const stateActions = useGridStateActions({
    stateRef,
    dispatch,
    statusFilter,
    folderId,
    collectionEntityId,
  });

  const itemActions = useGridItemActions({
    state,
    stateRef,
    imagesRef,
    singleSelectedHash,
    viewer,
    selectedScopeCount,
  });

  const imageClick = useGridImageClick({
    dispatch,
    viewer,
    stateRef,
    imagesRef,
    lastClickedHashRef,
    canvasLayoutRef,
    dismissHoverPreviewRef,
    dismissVideoScrubRef,
  });

  const reorder = useGridReorder({
    folderId,
    collectionEntityId,
    dispatch,
    stateRef,
    requestReplace,
    displayFolderId,
  });

  const loadMore = useCallback(() => {
    if (!state.hasMore || !state.defaultGridCursor) return;
    void requestAppend();
  }, [requestAppend, state.defaultGridCursor, state.hasMore]);

  return {
    singleSelectedHash,
    ...stateActions,
    ...itemActions,
    ...imageClick,
    ...reorder,
    loadMore,
  };
}
