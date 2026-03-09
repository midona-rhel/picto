import { useEffect, useMemo, useRef, type Dispatch, type RefObject } from 'react';
import { useGridMetadataStore } from '../../../state/gridMetadataStore';
import { resolveGridEmptyContext } from '../gridEmptyContext';
import type { GridRuntimeAction } from '../runtime';
import type { MasonryImageItem } from '../shared';
import type { SmartFolderPredicate } from '../../smart-folders/components/types';
import type { ViewerHostController } from '../../viewer/hooks/useViewerHost';
import type { MediaViewControls, MediaViewState } from '../../viewer/hooks/useViewerHost';
import { FADE_SETTLE_MS } from '../runtime/gridTransitionPipeline';

export function useGridRefreshLifecycle(args: {
  dispatch: Dispatch<GridRuntimeAction>;
  viewMode: 'grid' | 'waterfall' | 'justified';
  targetSize: number;
  folderId?: number | null;
  collectionEntityId?: number | null;
  searchTags?: string[];
  excludedSearchTags?: string[];
  tagMatchMode?: 'all' | 'any' | 'exact' | null;
  smartFolderPredicate?: SmartFolderPredicate;
  filterFolderIds?: number[] | null;
  excludedFilterFolderIds?: number[] | null;
  folderMatchMode?: 'all' | 'any' | 'exact' | null;
  statusFilter?: string | null;
  queryKey: string;
  requestReplace: () => Promise<void>;
  refreshTrigger?: number;
  stateRef: { current: { images: MasonryImageItem[] } };
  initialLoadDone: { current: boolean };
  viewer: ViewerHostController;
  onMediaViewStateChange?: (state: MediaViewState | null, controls: MediaViewControls | null) => void;
  scrollRef: RefObject<HTMLDivElement | null>;
}) {
  const {
    dispatch,
    viewMode,
    targetSize,
    folderId,
    collectionEntityId,
    searchTags,
    excludedSearchTags,
    tagMatchMode,
    smartFolderPredicate,
    filterFolderIds,
    excludedFilterFolderIds,
    folderMatchMode,
    statusFilter,
    queryKey,
    requestReplace,
    refreshTrigger,
    stateRef,
    initialLoadDone,
    viewer,
    onMediaViewStateChange,
    scrollRef,
  } = args;

  const pendingGridRemovals = useGridMetadataStore((s) => s.pendingGridRemovals);
  const metadataInvalidatedHashes = useGridMetadataStore((s) => s.metadataInvalidatedHashes);
  const gridRefreshSeq = useGridMetadataStore((s) => s.gridRefreshSeq);

  const scopeKey = useMemo(() => JSON.stringify({
    searchTags: searchTags ?? [],
    excludedSearchTags: excludedSearchTags ?? [],
    tagMatchMode: tagMatchMode ?? null,
    smartFolderPredicate: smartFolderPredicate ? JSON.stringify(smartFolderPredicate) : null,
    folderId: folderId ?? null,
    collectionEntityId: collectionEntityId ?? null,
    filterFolderIds: filterFolderIds ?? [],
    excludedFilterFolderIds: excludedFilterFolderIds ?? [],
    folderMatchMode: folderMatchMode ?? null,
    statusFilter: statusFilter ?? null,
  }), [
    collectionEntityId,
    excludedFilterFolderIds,
    excludedSearchTags,
    filterFolderIds,
    folderId,
    folderMatchMode,
    searchTags,
    smartFolderPredicate,
    statusFilter,
    tagMatchMode,
  ]);

  useEffect(() => {
    dispatch({
      type: 'COMMIT_GEOMETRY',
      viewMode,
      targetSize,
      folderId: folderId ?? null,
      searchTags,
      emptyContext: resolveGridEmptyContext(smartFolderPredicate, folderId, statusFilter),
    });
  }, [dispatch, folderId, searchTags, smartFolderPredicate, statusFilter, targetSize, viewMode]);

  // ---------------------------------------------------------------------------
  // Refs to avoid re-triggering effects on unstable identity values
  // ---------------------------------------------------------------------------

  const viewerRef = useRef(viewer);
  viewerRef.current = viewer;
  const onMediaViewStateChangeRef = useRef(onMediaViewStateChange);
  onMediaViewStateChangeRef.current = onMediaViewStateChange;
  const requestReplaceRef = useRef(requestReplace);
  requestReplaceRef.current = requestReplace;

  // ---------------------------------------------------------------------------
  // Transition orchestration
  // ---------------------------------------------------------------------------

  // Track previous scopeKey and queryKey to detect which changed
  const prevScopeKeyRef = useRef(scopeKey);
  const prevQueryKeyRef = useRef(queryKey);
  const fadeOutTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const fadeInTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const isFirstRenderRef = useRef(true);

  useEffect(() => {
    // On first render, just do a plain load — no fade
    if (isFirstRenderRef.current) {
      isFirstRenderRef.current = false;
      prevScopeKeyRef.current = scopeKey;
      prevQueryKeyRef.current = queryKey;
      initialLoadDone.current = false;
      void requestReplaceRef.current();
      return;
    }

    const scopeChanged = prevScopeKeyRef.current !== scopeKey;
    const queryChanged = prevQueryKeyRef.current !== queryKey;

    prevScopeKeyRef.current = scopeKey;
    prevQueryKeyRef.current = queryKey;

    // Nothing changed — skip
    if (!scopeChanged && !queryChanged) return;

    // Clear any pending transition timers (rapid clicks restart the fade)
    if (fadeOutTimerRef.current) {
      clearTimeout(fadeOutTimerRef.current);
      fadeOutTimerRef.current = null;
    }
    if (fadeInTimerRef.current) {
      clearTimeout(fadeInTimerRef.current);
      fadeInTimerRef.current = null;
    }

    // 1. Lock grid immediately — no debounce
    dispatch({ type: 'SET_TRANSITION_STAGE', stage: 'fading_out' });

    // 2. After fade-out completes, swap data and fade in
    fadeOutTimerRef.current = setTimeout(async () => {
      fadeOutTimerRef.current = null;

      if (scopeChanged) {
        // Scope change: close viewer, clear selection, reset scroll
        viewerRef.current.close('');
        onMediaViewStateChangeRef.current?.(null, null);
        dispatch({ type: 'CLEAR_SELECTION' });
        dispatch({ type: 'SET_SELECTED_SUBFOLDER', id: null });
        dispatch({ type: 'CLEAR_DATASET' });
        if (scrollRef.current) {
          scrollRef.current.scrollTop = 0;
        }
      }

      // Replace data while grid is invisible
      initialLoadDone.current = false;
      await requestReplaceRef.current();

      // 3. Start fade-in
      dispatch({ type: 'SET_TRANSITION_STAGE', stage: 'fading_in' });

      // 4. After fade-in completes, return to idle
      fadeInTimerRef.current = setTimeout(() => {
        fadeInTimerRef.current = null;
        dispatch({ type: 'SET_TRANSITION_STAGE', stage: 'idle' });
      }, FADE_SETTLE_MS);
    }, FADE_SETTLE_MS);

    return () => {
      if (fadeOutTimerRef.current) {
        clearTimeout(fadeOutTimerRef.current);
        fadeOutTimerRef.current = null;
      }
      if (fadeInTimerRef.current) {
        clearTimeout(fadeInTimerRef.current);
        fadeInTimerRef.current = null;
      }
    };
  }, [dispatch, scopeKey, queryKey, initialLoadDone, scrollRef]);

  // ---------------------------------------------------------------------------
  // External refresh triggers (gridRefreshSeq, refreshTrigger)
  // These do a plain requestReplace — no fade transition
  // ---------------------------------------------------------------------------

  const prevRefreshTrigger = useRef(refreshTrigger);
  useEffect(() => {
    if (prevRefreshTrigger.current !== refreshTrigger) {
      prevRefreshTrigger.current = refreshTrigger;
      void requestReplace();
    }
  }, [refreshTrigger, requestReplace]);

  // ---------------------------------------------------------------------------
  // Optimistic grid removals
  // ---------------------------------------------------------------------------

  useEffect(() => {
    if (pendingGridRemovals.size === 0) return;
    const toRemove = new Set(pendingGridRemovals);
    useGridMetadataStore.getState().clearGridRemovals();
    dispatch({ type: 'FILTER_IMAGES', predicate: (img) => !toRemove.has(img.hash) });
    dispatch({ type: 'REMOVE_HASHES', hashes: toRemove });
  }, [dispatch, pendingGridRemovals]);

  // ---------------------------------------------------------------------------
  // Active grid scope tracking
  // ---------------------------------------------------------------------------

  useEffect(() => {
    let scope: string;
    if (collectionEntityId != null) scope = `collection:${collectionEntityId}`;
    else if (folderId != null) scope = `folder:${folderId}`;
    else if (statusFilter === 'inbox') scope = 'system:inbox';
    else if (statusFilter === 'trash') scope = 'system:trash';
    else scope = 'system:all';
    useGridMetadataStore.getState().setActiveGridScope(scope);
  }, [collectionEntityId, folderId, statusFilter]);

  // ---------------------------------------------------------------------------
  // Metadata invalidation — patch tiles in-place
  // ---------------------------------------------------------------------------

  useEffect(() => {
    if (metadataInvalidatedHashes.size === 0) return;
    const hashes = [...metadataInvalidatedHashes];
    useGridMetadataStore.getState().clearInvalidatedHashes();

    useGridMetadataStore.getState().fetchMetadataBatch(hashes).then((results) => {
      if (results.length === 0) return;
      const metaMap = new Map(results.map((r) => [r.file.hash, r.file]));
      const currentImages = stateRef.current.images;
      let changed = false;
      const next = currentImages.map((img) => {
        const meta = metaMap.get(img.hash);
        if (!meta) return img;
        if (img.name === meta.name && img.rating === meta.rating && img.view_count === meta.view_count) {
          return img;
        }
        changed = true;
        return { ...img, name: meta.name, rating: meta.rating, view_count: meta.view_count };
      });
      if (changed) dispatch({ type: 'SET_IMAGES', images: next });
    });
  }, [dispatch, metadataInvalidatedHashes, stateRef]);

  // ---------------------------------------------------------------------------
  // Grid refresh sequence (from backend invalidation)
  // ---------------------------------------------------------------------------

  const prevGridRefreshSeq = useRef(gridRefreshSeq);
  useEffect(() => {
    if (prevGridRefreshSeq.current !== gridRefreshSeq) {
      prevGridRefreshSeq.current = gridRefreshSeq;
      void requestReplace();
    }
  }, [gridRefreshSeq, requestReplace]);
}
