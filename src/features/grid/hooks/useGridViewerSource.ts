import { useCallback, useEffect, useRef } from 'react';
import type { ViewerHostController } from '../../../features/viewer/hooks/useViewerHost';
import type { MediaViewControls, MediaViewState } from '../../../features/viewer/hooks/useViewerHost';
import type { MediaItem } from '../shared';

export function useGridViewerSource(args: {
  viewer: ViewerHostController;
  images: MediaItem[];
  totalCount: number;
  statusFilter?: string | null;
  handleInboxAction: ((hash: string, status: 'active' | 'trash') => void) | undefined;
  onMediaViewStateChange?: ((state: MediaViewState | null, controls: MediaViewControls | null) => void) | undefined;
  dispatch: React.Dispatch<any>;
  scrollToIndex: (index: number) => void;
  imagesRef: React.MutableRefObject<MediaItem[]>;
}) {
  const {
    viewer,
    images,
    totalCount,
    statusFilter,
    handleInboxAction,
    onMediaViewStateChange,
    dispatch,
    scrollToIndex,
    imagesRef,
  } = args;

  // Keep registerSource in a ref so the effect below doesn't re-fire when
  // viewer mode/session changes (which would rebase a not-yet-committed session).
  const registerSourceRef = useRef(viewer.registerSource);
  registerSourceRef.current = viewer.registerSource;

  const handleViewerDetailImageChange = useCallback((hash: string) => {
    dispatch({ type: 'SELECT_HASHES', hashes: new Set([hash]) });
    dispatch({ type: 'SET_LAST_CLICKED', hash });
  }, [dispatch]);

  const handleViewerQuickLookOpen = useCallback((_hash: string) => {
    // no-op after view count removal
  }, []);

  const handleViewerQuickLookImageChange = useCallback((hash: string) => {
    dispatch({ type: 'SELECT_HASHES', hashes: new Set([hash]) });
    dispatch({ type: 'SET_LAST_CLICKED', hash });
    const idx = imagesRef.current.findIndex((item) => item.hash === hash);
    if (idx >= 0) scrollToIndex(idx);
  }, [dispatch, imagesRef, scrollToIndex]);

  const handleViewerCloseDetail = useCallback((exitHash: string) => {
    if (!exitHash) return;
    dispatch({ type: 'SELECT_HASHES', hashes: new Set([exitHash]) });
    dispatch({ type: 'SET_LAST_CLICKED', hash: exitHash });
  }, [dispatch]);

  const handleViewerCloseQuickLook = useCallback(() => {
    // Quick Look closing no longer drives pop animation in the grid.
  }, []);

  useEffect(() => {
    registerSourceRef.current({
      images,
      totalCount,
      inboxMode: statusFilter === 'inbox',
      onInboxAction: statusFilter === 'inbox' ? handleInboxAction : undefined,
      onDetailStateChange: onMediaViewStateChange,
      onDetailImageChange: handleViewerDetailImageChange,
      onQuickLookOpen: handleViewerQuickLookOpen,
      onQuickLookImageChange: handleViewerQuickLookImageChange,
      onCloseDetail: handleViewerCloseDetail,
      onCloseQuickLook: handleViewerCloseQuickLook,
    });
  }, [
    handleInboxAction,
    handleViewerCloseDetail,
    handleViewerCloseQuickLook,
    handleViewerDetailImageChange,
    handleViewerQuickLookImageChange,
    handleViewerQuickLookOpen,
    images,
    onMediaViewStateChange,
    statusFilter,
    totalCount,
  ]);
}
