import { useCallback, useEffect, useLayoutEffect, useMemo, useRef } from 'react';
import type { SmartFolderPredicate } from '../../../features/smart-folders/components/types';
import { predicateToRust } from '../../../features/smart-folders/components/types';
import type { DetailViewControls, DetailViewState } from '../DetailView';
import {
  type GridRuntimeAction,
  type GridRuntimeState,
  type GridEmptyContext,
  isGridFrozen,
  FADE_SETTLE_MS,
  SCOPE_COALESCE_MS,
} from '../runtime';
import type { GridQueryBroker } from '../queryBroker/GridQueryBroker';
import type { GridQueryKey } from '../queryBroker/gridQueryKey';

interface UseGridTransitionControllerArgs {
  state: GridRuntimeState;
  dispatch: React.Dispatch<GridRuntimeAction>;
  broker: GridQueryBroker;
  queryKeyRef: { current: GridQueryKey };
  externalFreeze: boolean;
  viewMode: GridRuntimeState['displayViewMode'];
  targetSize: number;
  folderId: number | null | undefined;
  collectionEntityId: number | null | undefined;
  filterFolderIds: number[] | null | undefined;
  excludedFilterFolderIds: number[] | null | undefined;
  folderMatchMode: 'all' | 'any' | 'exact' | null;
  statusFilter: string | null | undefined;
  searchTags: string[] | undefined;
  excludedSearchTags: string[] | undefined;
  tagMatchMode: 'all' | 'any' | 'exact' | null;
  smartFolderPredicate: SmartFolderPredicate | null | undefined;
  onDetailViewStateChange?: ((state: DetailViewState | null, controls: DetailViewControls | null) => void) | undefined;
  onScopeTransitionMidpoint?: (() => void) | undefined;
  resolveEmptyContext: (
    smartFolderPredicate: SmartFolderPredicate | null | undefined,
    folderId: number | null | undefined,
    statusFilter: string | null | undefined,
  ) => GridEmptyContext;
}

interface GridTransitionControllerResult {
  gridFreezeActive: boolean;
  handleGridTransitionEnd: (e: React.TransitionEvent<HTMLDivElement>) => void;
}

export function useGridTransitionController({
  state,
  dispatch,
  broker,
  queryKeyRef,
  externalFreeze,
  viewMode,
  targetSize,
  folderId,
  collectionEntityId,
  filterFolderIds,
  excludedFilterFolderIds,
  folderMatchMode,
  statusFilter,
  searchTags,
  excludedSearchTags,
  tagMatchMode,
  smartFolderPredicate,
  onDetailViewStateChange,
  onScopeTransitionMidpoint,
  resolveEmptyContext,
}: UseGridTransitionControllerArgs): GridTransitionControllerResult {
  // Transition action ownership lives in this controller hook and is enforced
  // by scripts/ci/check-grid-architecture.mjs.
  const stateRef = useRef(state);
  stateRef.current = state;

  const gridFreezeActive = isGridFrozen(state, externalFreeze);
  const pendingScopeTransitionTimerRef = useRef<number | null>(null);
  const pendingScopeTransitionKeyRef = useRef<string | null>(null);
  const transitionSerialRef = useRef(0);
  const smartFolderScopeKey = useMemo(
    () => (smartFolderPredicate ? JSON.stringify(predicateToRust(smartFolderPredicate)) : 'none'),
    [smartFolderPredicate],
  );
  const scopeTransitionKey = useMemo(
    () =>
      JSON.stringify({
        searchTags: searchTags ?? [],
        smartFolder: smartFolderScopeKey,
        folderId: folderId ?? null,
        collectionEntityId: collectionEntityId ?? null,
        filterFolderIds: filterFolderIds ?? [],
        excludedFilterFolderIds: excludedFilterFolderIds ?? [],
        folderMatchMode,
        statusFilter: statusFilter ?? null,
        excludedSearchTags: excludedSearchTags ?? [],
        tagMatchMode,
      }),
    [searchTags, excludedSearchTags, tagMatchMode, smartFolderScopeKey, folderId, collectionEntityId, filterFolderIds, excludedFilterFolderIds, folderMatchMode, statusFilter],
  );
  const lastScopeTransitionKeyRef = useRef<string | null>(null);
  const desiredViewModeRef = useRef(viewMode);
  desiredViewModeRef.current = viewMode;
  const desiredTargetSizeRef = useRef(targetSize);
  desiredTargetSizeRef.current = targetSize;
  const desiredFolderIdRef = useRef(folderId ?? null);
  desiredFolderIdRef.current = folderId ?? null;
  const desiredSearchTagsRef = useRef(searchTags);
  desiredSearchTagsRef.current = searchTags;
  const desiredEmptyContextRef = useRef(resolveEmptyContext(smartFolderPredicate, folderId, statusFilter));
  desiredEmptyContextRef.current = resolveEmptyContext(smartFolderPredicate, folderId, statusFilter);

  const runTransition = useCallback(
    async (
      loadFn: () => Promise<void>,
      opts: {
        minFadeMs: number;
        applyGeometryAtCommit?: boolean;
        expectDeferredPayload?: boolean;
        cancelledRef: { current: boolean };
        onMidpoint?: () => void;
      },
    ) => {
      const runSerial = ++transitionSerialRef.current;
      const {
        minFadeMs,
        applyGeometryAtCommit = true,
        expectDeferredPayload = false,
        cancelledRef,
        onMidpoint,
      } = opts;
      const t0 = performance.now();
      dispatch({ type: 'BEGIN_FADE_OUT' });
      broker.armDeferredCommit();

      const fadePromise = new Promise<void>((resolve) => {
        const t = window.setTimeout(resolve, minFadeMs);
        if (cancelledRef.current) window.clearTimeout(t);
      });

      try {
        await Promise.all([loadFn(), fadePromise]);
        if (cancelledRef.current || runSerial !== transitionSerialRef.current) return;

        onMidpoint?.();

        const deferred = broker.takeDeferredPayload();

        const geometry = applyGeometryAtCommit
          ? {
              viewMode: desiredViewModeRef.current,
              targetSize: desiredTargetSizeRef.current,
              folderId: desiredFolderIdRef.current,
              searchTags: desiredSearchTagsRef.current,
              emptyContext: desiredEmptyContextRef.current,
            }
          : undefined;

        dispatch({
          type: 'COMMIT_TRANSITION',
          payload: deferred,
          geometry,
          clearIfNoPayload: expectDeferredPayload,
        });

        if (typeof localStorage !== 'undefined' && localStorage.getItem('picto:gridDebug') === '1') {
          const elapsed = (performance.now() - t0).toFixed(0);
          console.log(
            `[grid:transition] scope-switch #${transitionSerialRef.current} — fade-out → commit → fade-in (${elapsed}ms)`,
          );
        }

        broker.disarmDeferredCommit();
        // If the deferred payload was missing (cancelled transition) OR
        // a background event requested a reload during the transition,
        // fire a fresh replace now that the transition has committed.
        if ((!deferred && expectDeferredPayload) || broker.popReloadAfterTransition()) {
          broker.requestReplace(queryKeyRef.current);
        }
      } catch {
        if (runSerial === transitionSerialRef.current) {
          broker.disarmDeferredCommit();
        }
        if (!cancelledRef.current && runSerial === transitionSerialRef.current) {
          dispatch({ type: 'ABORT_TRANSITION' });
        }
      }
    },
    [dispatch, broker, queryKeyRef],
  );

  const handleGridTransitionEnd = useCallback(
    (e: React.TransitionEvent<HTMLDivElement>) => {
      if (
        stateRef.current.transitionStage === 'fading_in' &&
        e.target === e.currentTarget &&
        e.propertyName === 'opacity'
      ) {
        dispatch({ type: 'END_FADE' });
      }
    },
    [dispatch],
  );

  useLayoutEffect(() => {
    if (
      lastScopeTransitionKeyRef.current === scopeTransitionKey ||
      pendingScopeTransitionKeyRef.current === scopeTransitionKey
    ) {
      return;
    }

    if (pendingScopeTransitionTimerRef.current != null) {
      window.clearTimeout(pendingScopeTransitionTimerRef.current);
      pendingScopeTransitionTimerRef.current = null;
    }
    pendingScopeTransitionKeyRef.current = scopeTransitionKey;
    broker.armDeferredCommit();

    const cancelledRef = { current: false };
    pendingScopeTransitionTimerRef.current = window.setTimeout(() => {
      pendingScopeTransitionTimerRef.current = null;
      pendingScopeTransitionKeyRef.current = null;
      if (cancelledRef.current) return;
      lastScopeTransitionKeyRef.current = scopeTransitionKey;

      dispatch({ type: 'CLEAR_SELECTION' });
      dispatch({ type: 'SET_SELECTED_SUBFOLDER', id: null });
      dispatch({ type: 'CLOSE_DETAIL' });
      dispatch({ type: 'CLOSE_QUICK_LOOK' });
      onDetailViewStateChange?.(null, null);
      dispatch({ type: 'SET_CURSOR', cursor: null, hasMore: true });

      void runTransition(() => broker.requestReplaceAsync(queryKeyRef.current), {
        minFadeMs: FADE_SETTLE_MS,
        applyGeometryAtCommit: true,
        expectDeferredPayload: true,
        cancelledRef,
        onMidpoint: onScopeTransitionMidpoint,
      });
    }, SCOPE_COALESCE_MS);

    return () => {
      cancelledRef.current = true;
      if (pendingScopeTransitionTimerRef.current != null) {
        window.clearTimeout(pendingScopeTransitionTimerRef.current);
        pendingScopeTransitionTimerRef.current = null;
      }
      if (pendingScopeTransitionKeyRef.current === scopeTransitionKey) {
        pendingScopeTransitionKeyRef.current = null;
      }
      broker.disarmDeferredCommit();
    };
  }, [scopeTransitionKey, onDetailViewStateChange, onScopeTransitionMidpoint, runTransition, broker, dispatch, queryKeyRef]);

  // Geometry effect — crossfades viewMode switches, applies zoom changes instantly.
  // Reads transitionStage/displayViewMode/displayTargetSize from stateRef (not state)
  // to avoid re-running when the transition itself updates those values. That prevents
  // the effect cleanup from cancelling the in-flight transition it just started.
  // desiredViewModeRef/desiredTargetSizeRef ensure the commit always uses the latest
  // desired values even if the user changes them mid-transition.
  useLayoutEffect(() => {
    if (stateRef.current.transitionStage !== 'idle' || pendingScopeTransitionKeyRef.current) return;

    const viewModeChanged = viewMode !== stateRef.current.displayViewMode;
    const targetSizeChanged = targetSize !== stateRef.current.displayTargetSize;
    if (!viewModeChanged && !targetSizeChanged) return;

    if (viewModeChanged) {
      void runTransition(() => Promise.resolve(), {
        minFadeMs: FADE_SETTLE_MS,
        applyGeometryAtCommit: true,
        expectDeferredPayload: false,
        cancelledRef: { current: false },
      });
      return;
    }

    dispatch({
      type: 'COMMIT_GEOMETRY',
      viewMode,
      targetSize,
      folderId: desiredFolderIdRef.current,
      searchTags: desiredSearchTagsRef.current,
      emptyContext: desiredEmptyContextRef.current,
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [viewMode, targetSize, dispatch, runTransition]);

  useEffect(() => {
    return () => {
      if (pendingScopeTransitionTimerRef.current != null) {
        window.clearTimeout(pendingScopeTransitionTimerRef.current);
        pendingScopeTransitionTimerRef.current = null;
      }
      pendingScopeTransitionKeyRef.current = null;
    };
  }, []);

  return { gridFreezeActive, handleGridTransitionEnd };
}
