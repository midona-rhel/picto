import { useEffect, useMemo, useRef, useState, type Dispatch } from 'react';
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

export interface GridShellInitialScroll {
  top: number;
  token: number;
}

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
  fetchReplace: () => Promise<GridReplacePayload>;
  commitReplace: (payload: GridReplacePayload) => void;
  buildCommittedSurface: (payload: GridReplacePayload, scopeChanged: boolean) => GridSurfaceModel;
  initialLoadDone: { current: boolean };
  viewer: ViewerHostController;
  dispatch: Dispatch<GridRuntimeAction>;
  onMediaViewStateChange?: (state: MediaViewState | null, controls: MediaViewControls | null) => void;
  consumeScrollRestore: () => number | null;
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
  } = args;

  const [renderedScopeKey, setRenderedScopeKey] = useState(incomingScopeKey);
  const [renderedSurface, setRenderedSurface] = useState(liveSurface);
  const [transitionPhase, setTransitionPhase] = useState<GridSwapTransitionPhase>('idle');
  const [navigationMode, setNavigationMode] = useState<GridSwapNavigationMode>('steady');
  const [visibleTransitionStage, setVisibleTransitionStage] = useState<TransitionStage>('idle');
  const [shellInitialScroll, setShellInitialScroll] = useState<GridShellInitialScroll | null>(null);

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

  const prevIncomingScopeKeyRef = useRef(incomingScopeKey);
  const prevQueryKeyRef = useRef(queryKey);
  const isFirstRenderRef = useRef(true);
  const swapSequenceRef = useRef(0);
  const shellSwapTokenRef = useRef(0);
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
    if (renderedScopeKey !== liveSurface.scopeKey) {
      setRenderedScopeKey(liveSurface.scopeKey);
    }
    if (!equalGridSurfaceModel(renderedSurfaceRef.current, liveSurface)) {
      setRenderedSurface(liveSurface);
    }
  }, [liveSurface, navigationMode, renderedScopeKey, transitionPhase]);

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

    const restoreScroll = scopeChanged ? consumeScrollRestore() : null;
    const targetScrollTop = restoreScroll ?? 0;
    const nextNavigationMode: GridSwapNavigationMode = scopeChanged
      ? (restoreScroll != null ? 'history_restore' : 'fresh_scope_nav')
      : 'same_scope_query_change';
    const pendingReplace = requestReplaceRef.current();

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
        setShellInitialScroll(null);
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

        if (scopeChanged) {
          viewerRef.current.close('');
          onMediaViewStateChangeRef.current?.(null, null);
          dispatch({ type: 'CLEAR_SELECTION' });
          dispatch({ type: 'SET_SELECTED_SUBFOLDER', id: null });
        }

        dispatch({
          type: 'COMMIT_GEOMETRY',
          viewMode,
          targetSize,
          folderId: folderId ?? null,
          searchTags,
          emptyContext: resolveGridEmptyContext(smartFolderPredicate, folderId, statusFilter),
        });

        initialLoadDone.current = false;
        commitReplaceRef.current(payload);
        setRenderedSurface(buildCommittedSurfaceRef.current(payload, scopeChanged));

        if (scopeChanged) {
          const token = ++shellSwapTokenRef.current;
          setRenderedScopeKey(incomingScopeKey);
          setShellInitialScroll({ top: targetScrollTop, token });
          setTransitionPhase('shell_swapped_hidden');
        } else {
          setTransitionPhase('loading_new_data');
        }

        beginFadeIn();
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
    shellInitialScroll,
    preserveScrollBehaviors,
    visibleTransitionStage,
  };
}
