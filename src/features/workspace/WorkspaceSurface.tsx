import { useCallback, useEffect, useRef, useState } from 'react';
import { getDefaultStore, useAtomValue, useSetAtom } from 'jotai';
import { activeNodeIdAtom, displayedSurfaceNodeIdAtom, skipFadeOutAtom } from '../../state/navigation';
import { gridDrilldownAtom, gridTransitionPhaseAtom, pendingGridIntentAtom, pendingGridNavigationAtom, type GridTransitionPhase } from '../../state/grid';
import { gridController, type PreparedGridNavigation } from '../../controllers/gridController';
import { closeTransientViewers, getScrollPosition, saveScrollPosition } from '../../state/navigationHistory';
import { viewerExitTransitionAtom } from '../../state/viewer';
import { nodeIdToGridScope } from '../../shared/lib/gridScope';
import { ManagerSurface } from '../managers/ManagerSurface';
import { GridScreen } from '../grid/GridScreen';
import styles from '../grid/GridScreen.module.css';
import type { GridScrollPosition } from '../../shared/types/gridScroll';
import { gridDefaultSpacingAtom } from '../../state/grid';
import { gridGapForSpacing, GRID_TEXT_ROW_HEIGHT } from '../grid/gridAppearance';
import { prepareGridLayout, type GridLayoutConfig } from '../grid/canvas/gridLayoutModel';
import { beginGridCommit } from '../grid/gridBenchmarkTiming';

const store = getDefaultStore();
const TRANSITION_MS = 170;
const TOP_SCROLL_POSITION: GridScrollPosition = { scrollTop: 0, progress: 0 };

/** One owner for committing and animating application content surfaces. */
export function WorkspaceSurface() {
  const activeNodeId = useAtomValue(activeNodeIdAtom);
  const drilldown = useAtomValue(gridDrilldownAtom);
  const activeScopeNodeId = drilldown?.ownerNodeId === activeNodeId
    ? drilldown.scopeNodeId
    : activeNodeId;
  const activeGridScope = nodeIdToGridScope(activeScopeNodeId);
  const displayedNodeId = useAtomValue(displayedSurfaceNodeIdAtom);
  const [displayedDrilldown, setDisplayedDrilldown] = useState<{
    ownerNodeId: string;
    scopeNodeId: string;
  } | null>(() => drilldown ? {
    ownerNodeId: drilldown.ownerNodeId,
    scopeNodeId: drilldown.scopeNodeId,
  } : null);
  const displayedScopeNodeId = displayedDrilldown?.ownerNodeId === displayedNodeId
    ? displayedDrilldown.scopeNodeId
    : displayedNodeId;
  const pendingIntent = useAtomValue(pendingGridIntentAtom);
  const setDisplayedNodeId = useSetAtom(displayedSurfaceNodeIdAtom);
  const setPendingIntent = useSetAtom(pendingGridIntentAtom);
  const [phase, setPhaseState] = useState<GridTransitionPhase>('idle');
  const [inlineScrollRestore, setInlineScrollRestore] = useState<{
    token: number;
    nodeId: string;
    position: GridScrollPosition;
  } | null>(null);
  const inlineRestoreTokenRef = useRef(0);
  const phaseRef = useRef<GridTransitionPhase>('idle');
  const initializedRef = useRef(false);
  const previousNodeRef = useRef(activeScopeNodeId);
  const pendingNodeRef = useRef(activeScopeNodeId);
  const pendingOwnerNodeRef = useRef(activeNodeId);
  const prefetchRef = useRef<{
    nodeId: string;
    ownerNodeId: string;
    generation: number;
    promise: Promise<PreparedGridNavigation>;
  } | null>(null);
  const prefetchGenerationRef = useRef(0);
  const layoutConfigRef = useRef<GridLayoutConfig | null>(null);
  const captureLayoutConfig = useCallback((config: GridLayoutConfig) => { layoutConfigRef.current = config; }, []);
  const warmLayout = useCallback((prepared: PreparedGridNavigation, generation: number) => {
    const config = layoutConfigRef.current;
    if (config && generation === prefetchGenerationRef.current && prepared.session.status === 'idle') {
      const view = prepared.session.view;
      prepareGridLayout(prepared.session.items, { ...config,
        viewMode: view.mode, targetSize: view.targetSize,
        gap: gridGapForSpacing(view.spacing ?? store.get(gridDefaultSpacingAtom)),
        textHeight: (Number(view.showName) + Number(view.showResolution)) * GRID_TEXT_ROW_HEIGHT });
    }
    return prepared;
  }, []);
  const [committedGeneration, setCommittedGeneration] = useState(0);
  const scrollPositionRef = useRef<GridScrollPosition>({ scrollTop: 0, progress: 0 });
  const restoreScrollPositionRef = useRef<GridScrollPosition | null>(null);
  const delayRef = useRef<number | null>(null);
  const frameRef = useRef<number | null>(null);

  const setPhase = useCallback((next: GridTransitionPhase) => {
    if (phaseRef.current === next) return;
    phaseRef.current = next;
    store.set(gridTransitionPhaseAtom, next);
    setPhaseState(next);
  }, []);

  const cancelSchedule = useCallback(() => {
    if (delayRef.current != null) window.clearTimeout(delayRef.current);
    if (frameRef.current != null) window.cancelAnimationFrame(frameRef.current);
    delayRef.current = null;
    frameRef.current = null;
  }, []);

  const consumeNavigationOptions = useCallback((nodeId: string) => {
    const pending = store.get(pendingGridNavigationAtom);
    if (pending?.nodeId === nodeId) {
      store.set(pendingGridNavigationAtom, null);
      return pending.sort
        ? { filters: pending.filters, sort: pending.sort }
        : { filters: pending.filters };
    }
    return undefined;
  }, []);

  const navigateGrid = useCallback((nodeId: string, scope: NonNullable<typeof activeGridScope>) => {
    const options = consumeNavigationOptions(nodeId);
    return options ? gridController.navigateTo(scope, options) : gridController.navigateTo(scope);
  }, [consumeNavigationOptions]);

  const prepareScrollRestore = useCallback((nodeId: string) => {
    const pending = store.get(pendingGridNavigationAtom);
    restoreScrollPositionRef.current = pending?.restoreScroll
      ? getScrollPosition(nodeId) ?? TOP_SCROLL_POSITION
      : TOP_SCROLL_POSITION;
  }, []);

  const prefetchGrid = useCallback((
    nodeId: string,
    ownerNodeId: string,
    scope: NonNullable<typeof activeGridScope>,
  ) => {
    prepareScrollRestore(nodeId);
    const generation = ++prefetchGenerationRef.current;
    const options = consumeNavigationOptions(nodeId);
    const progress = restoreScrollPositionRef.current?.progress ?? 0;
    const preparationOptions = progress > 0 ? { ...options, restoreProgress: progress } : options;
    const query = preparationOptions
      ? gridController.prepareNavigation(scope, preparationOptions)
      : gridController.prepareNavigation(scope);
    const promise = query.then(prepared => warmLayout(prepared, generation));
    prefetchRef.current = { nodeId, ownerNodeId, generation, promise };
    return promise;
  }, [consumeNavigationOptions, prepareScrollRestore, warmLayout]);

  const fadeIn = useCallback(() => {
    if (phaseRef.current !== 'waiting') return;
    cancelSchedule();
    frameRef.current = requestAnimationFrame(() => {
      frameRef.current = null;
      setPhase('fading_in');
      delayRef.current = window.setTimeout(() => {
        delayRef.current = null;
        setPhase('idle');
        store.set(viewerExitTransitionAtom, false);
      }, TRANSITION_MS);
    });
  }, [cancelSchedule, setPhase]);

  const commitDestination = useCallback(() => {
    const committedNode = pendingNodeRef.current;
    const committedOwnerNode = pendingOwnerNodeRef.current;
    const committedScope = nodeIdToGridScope(committedNode);
    setPhase('waiting');
    if (!committedScope) {
      prefetchGenerationRef.current += 1;
      prefetchRef.current = null;
      closeTransientViewers();
      setDisplayedNodeId(committedOwnerNode);
      setDisplayedDrilldown(null);
      previousNodeRef.current = committedNode;
      gridController.deactivate();
      fadeIn();
      return;
    }
    const prefetched = prefetchRef.current;
    const promise = prefetched?.nodeId === committedNode
      && prefetched.ownerNodeId === committedOwnerNode
      ? prefetched.promise
      : prefetchGrid(committedNode, committedOwnerNode, committedScope);
    const generation = prefetchRef.current?.generation;
    void promise.then((prepared) => {
      if (
        prefetchRef.current?.generation === generation
        && phaseRef.current === 'waiting'
      ) {
        beginGridCommit(prepared.session.items);
        closeTransientViewers();
        setDisplayedNodeId(committedOwnerNode);
        setDisplayedDrilldown(committedOwnerNode !== committedNode ? {
          ownerNodeId: committedOwnerNode,
          scopeNodeId: committedNode,
        } : null);
        previousNodeRef.current = committedNode;
        gridController.commitNavigation(prepared);
        setCommittedGeneration(generation!);
      }
    });
  }, [fadeIn, prefetchGrid, setDisplayedNodeId, setPhase]);

  const startFadeOut = useCallback(() => {
    cancelSchedule();
    setPhase('fading_out');
    delayRef.current = window.setTimeout(() => {
      delayRef.current = null;
      commitDestination();
    }, TRANSITION_MS);
  }, [cancelSchedule, commitDestination, setPhase]);

  useEffect(() => {
    pendingNodeRef.current = activeScopeNodeId;
    pendingOwnerNodeRef.current = activeNodeId;
    const previousScope = nodeIdToGridScope(previousNodeRef.current);
    const nextScope = activeGridScope;

    // Re-renders for the current destination do not restart its load or
    // animation. Repeated clicks during fade-out only retarget the midpoint.
    if (
      phaseRef.current !== 'idle'
      && activeScopeNodeId === previousNodeRef.current
      && activeNodeId === displayedNodeId
    ) return;

    if (phaseRef.current === 'fading_out') {
      if (
        nextScope
        && (
          prefetchRef.current?.nodeId !== activeScopeNodeId
          || prefetchRef.current.ownerNodeId !== activeNodeId
        )
      ) void prefetchGrid(activeScopeNodeId, activeNodeId, nextScope);
      else {
        if (!nextScope) {
          prefetchGenerationRef.current += 1;
          prefetchRef.current = null;
        }
      }
      return;
    }

    cancelSchedule();

    if (phaseRef.current === 'waiting' || phaseRef.current === 'fading_in') {
      if (nextScope) void prefetchGrid(activeScopeNodeId, activeNodeId, nextScope);
      commitDestination();
      return;
    }

    if (activeScopeNodeId === previousNodeRef.current) {
      if (initializedRef.current) return;
      initializedRef.current = true;
      if (nextScope) void navigateGrid(activeScopeNodeId, nextScope);
      else gridController.deactivate();
      return;
    }
    initializedRef.current = true;
    if (previousScope) saveScrollPosition(previousNodeRef.current, scrollPositionRef.current);

    if (nextScope && previousScope && store.get(skipFadeOutAtom)) {
      store.set(skipFadeOutAtom, false);
      void prefetchGrid(activeScopeNodeId, activeNodeId, nextScope);
      commitDestination();
      return;
    }

    if (nextScope) void prefetchGrid(activeScopeNodeId, activeNodeId, nextScope);
    else {
      prefetchGenerationRef.current += 1;
      prefetchRef.current = null;
    }
    startFadeOut();
  }, [activeGridScope, activeNodeId, activeScopeNodeId, cancelSchedule, commitDestination, displayedNodeId, navigateGrid, prefetchGrid, startFadeOut]);

  useEffect(() => {
    if (!pendingIntent) return;
    if (pendingIntent.type === 'filter') {
      const position = pendingIntent.restoreScroll
        ? getScrollPosition(activeScopeNodeId) ?? TOP_SCROLL_POSITION
        : TOP_SCROLL_POSITION;
      const token = ++inlineRestoreTokenRef.current;
      restoreScrollPositionRef.current = position;
      setInlineScrollRestore({ token, nodeId: activeScopeNodeId, position });
      setPendingIntent(null);
      gridController.applyIntent(pendingIntent);
      return;
    }
    restoreScrollPositionRef.current = TOP_SCROLL_POSITION;
    setPendingIntent(null);
    const generation = ++prefetchGenerationRef.current;
    const promise = gridController.prepareIntent(pendingIntent).then(prepared => warmLayout(prepared, generation));
    prefetchRef.current = { nodeId: activeScopeNodeId, ownerNodeId: activeNodeId, generation, promise };
    if (phaseRef.current === 'idle') startFadeOut();
    else if (phaseRef.current !== 'fading_out') {
      cancelSchedule();
      commitDestination();
    }
  }, [activeNodeId, activeScopeNodeId, pendingIntent, setPendingIntent, startFadeOut, cancelSchedule, commitDestination, warmLayout]);

  useEffect(() => () => {
    cancelSchedule();
    // React's development setup/cleanup/setup cycle must start a fresh load
    // after canceling the first one, just like a real library remount.
    initializedRef.current = false;
    prefetchGenerationRef.current += 1;
    prefetchRef.current = null;
    gridController.cancelPreparation();
  }, [cancelSchedule]);

  const revealCommittedGrid = useCallback(() => {
    restoreScrollPositionRef.current = null;
    fadeIn();
  }, [fadeIn]);

  const className = phase === 'waiting' ? styles.surfaceIncomingHidden
    : phase === 'fading_out' ? styles.surfaceFadeOut
    : phase === 'fading_in' ? styles.surfaceIncomingFadeIn
    : styles.surfaceIncomingVisible;
  const gridScope = nodeIdToGridScope(displayedScopeNodeId);
  const destinationCommitted = phase === 'waiting'
    && prefetchRef.current?.generation === committedGeneration
    && displayedScopeNodeId === pendingNodeRef.current
    && displayedNodeId === pendingOwnerNodeRef.current;
  const inlineRestoreCommitted = inlineScrollRestore?.nodeId === displayedScopeNodeId;
  const initialScrollPosition = inlineRestoreCommitted
    ? inlineScrollRestore.position
    : destinationCommitted ? restoreScrollPositionRef.current : null;
  const finishInlineRestore = inlineRestoreCommitted ? () => {
    setInlineScrollRestore((current) => current?.token === inlineScrollRestore.token ? null : current);
    restoreScrollPositionRef.current = null;
  } : undefined;

  return (
    <div className={styles.root}>
      <div className={`${styles.surface} ${className}`} data-view-phase={phase}
        data-view-node={displayedScopeNodeId} data-view-generation={committedGeneration}>
        {gridScope ? (
          <GridScreen
            onLayoutConfigChange={captureLayoutConfig}
            nodeId={displayedScopeNodeId}
            transitionPhase={phase}
            initialScrollPosition={initialScrollPosition}
            onFirstPaint={finishInlineRestore ?? (destinationCommitted ? revealCommittedGrid : undefined)}
            onScrollPositionChange={(position) => {
              scrollPositionRef.current = position;
              saveScrollPosition(displayedScopeNodeId, position);
            }}
          />
        ) : <ManagerSurface nodeId={displayedNodeId} />}
      </div>
    </div>
  );
}
