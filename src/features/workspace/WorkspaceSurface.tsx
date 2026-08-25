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
    const promise = options
      ? gridController.prepareNavigation(scope, options)
      : gridController.prepareNavigation(scope);
    prefetchRef.current = { nodeId, ownerNodeId, generation, promise };
    return promise;
  }, [consumeNavigationOptions, prepareScrollRestore]);

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
        closeTransientViewers();
        setDisplayedNodeId(committedOwnerNode);
        setDisplayedDrilldown(committedOwnerNode !== committedNode ? {
          ownerNodeId: committedOwnerNode,
          scopeNodeId: committedNode,
        } : null);
        previousNodeRef.current = committedNode;
        gridController.commitNavigation(prepared);
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
    restoreScrollPositionRef.current = pendingIntent.type === 'filter' && pendingIntent.restoreScroll
      ? getScrollPosition(activeScopeNodeId) ?? TOP_SCROLL_POSITION
      : TOP_SCROLL_POSITION;
    if (pendingIntent.type === 'filter') {
      setPendingIntent(null);
      gridController.applyIntent(pendingIntent);
      return;
    }
    if (phase !== 'idle') {
      gridController.applyIntent(pendingIntent);
      setPendingIntent(null);
      return;
    }
    setPendingIntent(null);
    setPhase('fading_out');
    delayRef.current = window.setTimeout(() => {
      delayRef.current = null;
      gridController.applyIntent(pendingIntent);
      setPhase('waiting');
    }, TRANSITION_MS);
  }, [activeScopeNodeId, pendingIntent, phase, setPendingIntent, setPhase]);

  useEffect(() => cancelSchedule, [cancelSchedule]);

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
    && displayedScopeNodeId === pendingNodeRef.current
    && displayedNodeId === pendingOwnerNodeRef.current;

  return (
    <div className={styles.root}>
      <div className={`${styles.surface} ${className}`}>
        {gridScope ? (
          <GridScreen
            nodeId={displayedScopeNodeId}
            transitionPhase={phase}
            initialScrollPosition={destinationCommitted ? restoreScrollPositionRef.current : null}
            onFirstPaint={destinationCommitted ? revealCommittedGrid : undefined}
            onScrollPositionChange={(position) => { scrollPositionRef.current = position; }}
          />
        ) : <ManagerSurface nodeId={displayedNodeId} />}
      </div>
    </div>
  );
}
