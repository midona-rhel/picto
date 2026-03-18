import { useEffect, useMemo, useRef, useState, type Dispatch } from 'react';
import { useNavigationStore } from '../../../state/navigationStore';
import { resolveGridEmptyContext } from '../gridEmptyContext';
import type { GridReplacePayload } from './useGridData';
import type { ViewerHostController } from '../../viewer/hooks/useViewerHost';
import type { MediaViewControls, MediaViewState } from '../../viewer/hooks/useViewerHost';
import type { GridRuntimeAction } from '../runtime';
import type { GridViewMode } from '../runtime';
import type { SmartFolderPredicate } from '../../smart-folders/components/types';
import type { TransitionStage } from '../runtime/gridTransitionPipeline';
import { FADE_SETTLE_MS } from '../runtime/gridTransitionPipeline';
import {
  type GridSurfaceModel,
  equalGridSurfaceModel,
} from '../gridSurfaceModel';

export type GridSwapNavigationMode =
  | 'steady'
  | 'fresh_scope_nav'
  | 'history_restore'
  | 'same_scope_query_change';

export type GridSwapTransitionPhase =
  | 'idle'
  | 'fading_out_old'
  | 'shell_swapped_hidden'
  | 'loading_new_data'
  | 'fading_in_new';

export function useGridSwapController(args: {
  incomingScopeKey: string;
  queryKey: string;
  liveSurface: GridSurfaceModel;
  viewMode: GridViewMode;
  targetSize: number;
  folderId?: number | null;
  searchTags?: string[];
  smartFolderPredicate?: SmartFolderPredicate;
  statusFilter?: string | null;
  fetchReplace: (minItems?: number) => Promise<GridReplacePayload>;
  commitReplace: (payload: GridReplacePayload) => void;
  buildCommittedSurface: (payload: GridReplacePayload, scopeChanged: boolean) => GridSurfaceModel;
  initialLoadDone: { current: boolean };
  viewer: ViewerHostController;
  dispatch: Dispatch<GridRuntimeAction>;
  onMediaViewStateChange?: (state: MediaViewState | null, controls: MediaViewControls | null) => void;
  consumeScrollRestore: () => number | null;
  scrollRef: React.MutableRefObject<HTMLDivElement | null>;
}) {
  const {
    incomingScopeKey,
    queryKey,
    liveSurface,
    viewMode,
    targetSize,
    folderId,
    searchTags,
    smartFolderPredicate,
    statusFilter,
    fetchReplace,
    commitReplace,
    buildCommittedSurface,
    initialLoadDone,
    viewer,
    dispatch,
    onMediaViewStateChange,
    consumeScrollRestore,
    scrollRef,
  } = args;
  void scrollRef;

  const [renderedScopeKey, setRenderedScopeKey] = useState(incomingScopeKey);
  const [renderedSurface, setRenderedSurface] = useState(liveSurface);
  const [transitionPhase, setTransitionPhase] = useState<GridSwapTransitionPhase>('idle');
  const [navigationMode, setNavigationMode] = useState<GridSwapNavigationMode>('steady');
  const [visibleTransitionStage, setVisibleTransitionStage] = useState<TransitionStage>('idle');

  const requestReplaceRef = useRef(fetchReplace);
  requestReplaceRef.current = fetchReplace;
  const commitReplaceRef = useRef(commitReplace);
  commitReplaceRef.current = commitReplace;
  const buildCommittedSurfaceRef = useRef(buildCommittedSurface);
  buildCommittedSurfaceRef.current = buildCommittedSurface;
  const viewerRef = useRef(viewer);
  viewerRef.current = viewer;
  const onMediaViewStateChangeRef = useRef(onMediaViewStateChange);
  onMediaViewStateChangeRef.current = onMediaViewStateChange;
  const renderedSurfaceRef = useRef(renderedSurface);
  renderedSurfaceRef.current = renderedSurface;
  const viewModeRef = useRef(viewMode);
  viewModeRef.current = viewMode;
  const targetSizeRef = useRef(targetSize);
  targetSizeRef.current = targetSize;
  const folderIdRef = useRef(folderId);
  folderIdRef.current = folderId;
  const searchTagsRef = useRef(searchTags);
  searchTagsRef.current = searchTags;
  const smartFolderPredicateRef = useRef(smartFolderPredicate);
  smartFolderPredicateRef.current = smartFolderPredicate;
  const statusFilterRef = useRef(statusFilter);
  statusFilterRef.current = statusFilter;

  const prevIncomingScopeKeyRef = useRef(incomingScopeKey);
  const prevQueryKeyRef = useRef(queryKey);
  const isFirstRenderRef = useRef(true);
  const swapSequenceRef = useRef(0);
  const fadeOutTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const fadeInTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const preserveScrollBehaviors = useMemo(
    () => transitionPhase === 'idle',
    [transitionPhase],
  );

  useEffect(() => {
    return () => {
      swapSequenceRef.current += 1;
      if (fadeOutTimerRef.current) clearTimeout(fadeOutTimerRef.current);
      if (fadeInTimerRef.current) clearTimeout(fadeInTimerRef.current);
    };
  }, []);

  useEffect(() => {
    if (transitionPhase !== 'idle' || navigationMode !== 'steady') return;
    const hasPendingInputChange =
      prevIncomingScopeKeyRef.current !== incomingScopeKey
      || prevQueryKeyRef.current !== queryKey;
    if (hasPendingInputChange) return;
    if (renderedScopeKey !== liveSurface.scopeKey) {
      setRenderedScopeKey(liveSurface.scopeKey);
    }
    if (!equalGridSurfaceModel(renderedSurfaceRef.current, liveSurface)) {
      setRenderedSurface(liveSurface);
    }
  }, [incomingScopeKey, liveSurface, navigationMode, queryKey, renderedScopeKey, transitionPhase]);

  useEffect(() => {
    if (isFirstRenderRef.current) {
      isFirstRenderRef.current = false;
      prevIncomingScopeKeyRef.current = incomingScopeKey;
      prevQueryKeyRef.current = queryKey;
      dispatch({
        type: 'COMMIT_GEOMETRY',
        viewMode,
        targetSize,
        folderId: folderId ?? null,
        searchTags,
        emptyContext: resolveGridEmptyContext(smartFolderPredicate, folderId, statusFilter),
      });
      initialLoadDone.current = false;
      void requestReplaceRef.current().then((payload) => {
        commitReplaceRef.current(payload);
      });
      return;
    }

    const scopeChanged = prevIncomingScopeKeyRef.current !== incomingScopeKey;
    const queryChanged = prevQueryKeyRef.current !== queryKey;
    prevIncomingScopeKeyRef.current = incomingScopeKey;
    prevQueryKeyRef.current = queryKey;

    if (!scopeChanged && !queryChanged) return;

    swapSequenceRef.current += 1;
    const sequence = swapSequenceRef.current;

    if (fadeOutTimerRef.current) clearTimeout(fadeOutTimerRef.current);
    if (fadeInTimerRef.current) clearTimeout(fadeInTimerRef.current);
    fadeOutTimerRef.current = null;
    fadeInTimerRef.current = null;

    // Read but don't consume yet — the effect may re-run and we need the value to survive.
    const navState = useNavigationStore.getState();
    const pendingRestore = navState.pendingScrollRestore;
    const pendingItemCount = navState.pendingLoadedItemCount;
    const targetScrollTop = pendingRestore ?? 0;
    const nextNavigationMode: GridSwapNavigationMode = scopeChanged
      ? (pendingRestore != null ? 'history_restore' : 'fresh_scope_nav')
      : 'same_scope_query_change';
    const pendingReplace = requestReplaceRef.current(pendingItemCount > 0 ? pendingItemCount : undefined);

    const beginFadeIn = () => {
      if (swapSequenceRef.current !== sequence) return;
      setTransitionPhase('fading_in_new');
      setVisibleTransitionStage('fading_in');
      fadeInTimerRef.current = setTimeout(() => {
        if (swapSequenceRef.current !== sequence) return;
        fadeInTimerRef.current = null;
        setTransitionPhase('idle');
        setNavigationMode('steady');
        setVisibleTransitionStage('idle');
      }, FADE_SETTLE_MS);
    };

    setNavigationMode(nextNavigationMode);
    setTransitionPhase('fading_out_old');
    setVisibleTransitionStage('fading_out');

    fadeOutTimerRef.current = setTimeout(() => {
      fadeOutTimerRef.current = null;
      if (swapSequenceRef.current !== sequence) return;

      void pendingReplace.then((payload) => {
        if (swapSequenceRef.current !== sequence) return;

        viewerRef.current.close('');
        onMediaViewStateChangeRef.current?.(null, null);
        dispatch({ type: 'CLEAR_SELECTION' });
        dispatch({ type: 'SET_SELECTED_SUBFOLDER', id: null });

        dispatch({
          type: 'COMMIT_GEOMETRY',
          viewMode: viewModeRef.current,
          targetSize: targetSizeRef.current,
          folderId: folderIdRef.current ?? null,
          searchTags: searchTagsRef.current,
          emptyContext: resolveGridEmptyContext(smartFolderPredicateRef.current, folderIdRef.current, statusFilterRef.current),
        });

        initialLoadDone.current = false;
        commitReplaceRef.current(payload);
        setRenderedSurface(buildCommittedSurfaceRef.current(payload, scopeChanged));

        setRenderedScopeKey(incomingScopeKey);

        setVisibleTransitionStage('preparing');

        setTimeout(() => {
          if (swapSequenceRef.current !== sequence) return;
          const el = scrollRef.current;
          consumeScrollRestore();
          if (el) {
            el.scrollTop = targetScrollTop;
            // Force a scroll event so the canvas redraws at the new position.
            el.dispatchEvent(new Event('scroll'));
          }
          // One more frame for the canvas to draw at the restored position.
          requestAnimationFrame(() => {
            if (swapSequenceRef.current !== sequence) return;
            beginFadeIn();
          });
        }, 50);
      });
    }, FADE_SETTLE_MS);
  }, [
    consumeScrollRestore,
    dispatch,
    folderId,
    incomingScopeKey,
    initialLoadDone,
    queryKey,
    searchTags,
    smartFolderPredicate,
    statusFilter,
    targetSize,
    viewMode,
  ]);

  return {
    renderedScopeKey,
    renderedSurface,
    transitionPhase,
    navigationMode,
    preserveScrollBehaviors,
    visibleTransitionStage,
  };
}
