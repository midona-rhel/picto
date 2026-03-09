import { useCallback } from 'react';
import type { MutableRefObject } from 'react';
import type { LayoutItem } from '../layoutMath';
import type { MasonryImageItem } from '../shared';
import type { ViewerHostController } from '../../../features/viewer/hooks/useViewerHost';
import type { GridRuntimeAction, GridRuntimeState } from '../runtime';
import { useGridMutationActions } from './useGridMutationActions';
import { useGridItemActions } from './useGridItemActions';
import { useGridImageClick } from './useGridImageClick';
import { useGridReorder } from './useGridReorder';

export function useGridActionHandlers(args: {
  state: GridRuntimeState;
  stateRef: MutableRefObject<GridRuntimeState>;
  dispatch: React.Dispatch<GridRuntimeAction>;
  imagesRef: MutableRefObject<MasonryImageItem[]>;
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
  } = args;

  const singleSelectedHash = !state.virtualAllSelection && state.selectedHashes.size === 1
    ? [...state.selectedHashes][0]
    : null;

  const mutationActions = useGridMutationActions({
    stateRef,
    dispatch,
    statusFilter,
    folderId,
    collectionEntityId,
    requestGridReload: () => { void requestReplace(); },
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
    ...mutationActions,
    ...itemActions,
    ...imageClick,
    ...reorder,
    loadMore,
  };
}
