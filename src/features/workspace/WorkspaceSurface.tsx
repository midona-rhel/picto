import { useCallback, useEffect, useRef, useState } from 'react';
import { getDefaultStore, useAtomValue, useSetAtom } from 'jotai';
import { activeNodeIdAtom, displayedSurfaceNodeIdAtom, skipFadeOutAtom } from '../../state/navigation';
import { activeGridScopeAtom, gridLoadingAtom, gridTransitionPhaseAtom, pendingGridIntentAtom, type GridTransitionPhase } from '../../state/grid';
import { gridController } from '../../controllers/gridController';
import { getScrollPosition, saveScrollPosition } from '../../state/navigationHistory';
import { nodeIdToGridScope } from '../../shared/lib/gridScope';
import { ManagerSurface } from '../managers/ManagerSurface';
import { GridScreen } from '../grid/GridScreen';
import styles from '../grid/GridScreen.module.css';

const store = getDefaultStore();
const TRANSITION_MS = 170;

/** One owner for committing and animating application content surfaces. */
export function WorkspaceSurface() {
  const activeNodeId = useAtomValue(activeNodeIdAtom);
  const activeGridScope = useAtomValue(activeGridScopeAtom);
  const displayedNodeId = useAtomValue(displayedSurfaceNodeIdAtom);
  const loading = useAtomValue(gridLoadingAtom);
  const pendingIntent = useAtomValue(pendingGridIntentAtom);
  const setDisplayedNodeId = useSetAtom(displayedSurfaceNodeIdAtom);
  const setPendingIntent = useSetAtom(pendingGridIntentAtom);
  const [phase, setPhaseState] = useState<GridTransitionPhase>('idle');
  const phaseRef = useRef<GridTransitionPhase>('idle');
  const previousNodeRef = useRef(activeNodeId);
  const pendingNodeRef = useRef(activeNodeId);
  const scrollTopRef = useRef(0);
  const restoreScrollTopRef = useRef<number | null>(null);
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

  const fadeIn = useCallback(() => {
    if (phaseRef.current !== 'waiting') return;
    cancelSchedule();
    frameRef.current = requestAnimationFrame(() => {
      frameRef.current = null;
      setPhase('fading_in');
      delayRef.current = window.setTimeout(() => {
        delayRef.current = null;
        setPhase('idle');
      }, TRANSITION_MS);
    });
  }, [cancelSchedule, setPhase]);

  useEffect(() => {
    pendingNodeRef.current = activeNodeId;
    cancelSchedule();
    const previousScope = nodeIdToGridScope(previousNodeRef.current);
    const nextScope = activeGridScope;

    if (activeNodeId === previousNodeRef.current) return;
    if (previousScope) saveScrollPosition(previousNodeRef.current, scrollTopRef.current);

    if (nextScope && previousScope && store.get(skipFadeOutAtom)) {
      store.set(skipFadeOutAtom, false);
      restoreScrollTopRef.current = getScrollPosition(activeNodeId);
      previousNodeRef.current = activeNodeId;
      setPhase('waiting');
      void gridController.navigateTo(nextScope);
      return;
    }

    setPhase('fading_out');
    delayRef.current = window.setTimeout(() => {
      delayRef.current = null;
      const committedNode = pendingNodeRef.current;
      const committedScope = nodeIdToGridScope(committedNode);
      setDisplayedNodeId(committedNode);
      previousNodeRef.current = committedNode;
      setPhase('waiting');
      if (committedScope) {
        restoreScrollTopRef.current = getScrollPosition(committedNode);
        void gridController.navigateTo(committedScope);
      } else {
        gridController.deactivate();
        fadeIn();
      }
    }, TRANSITION_MS);
  }, [activeGridScope, activeNodeId, cancelSchedule, fadeIn, setDisplayedNodeId, setPhase]);

  useEffect(() => {
    if (phase === 'waiting' && !loading && nodeIdToGridScope(displayedNodeId)) fadeIn();
  }, [displayedNodeId, fadeIn, loading, phase]);

  useEffect(() => {
    if (!pendingIntent) return;
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
  }, [pendingIntent, phase, setPendingIntent, setPhase]);

  useEffect(() => cancelSchedule, [cancelSchedule]);

  const className = phase === 'waiting' ? styles.surfaceIncomingHidden
    : phase === 'fading_out' ? styles.surfaceFadeOut
    : phase === 'fading_in' ? styles.surfaceIncomingFadeIn
    : styles.surfaceIncomingVisible;
  const gridScope = nodeIdToGridScope(displayedNodeId);

  return (
    <div className={styles.root}>
      <div className={`${styles.surface} ${className}`}>
        {gridScope ? (
          <GridScreen
            nodeId={displayedNodeId}
            transitionPhase={phase}
            initialScrollTop={restoreScrollTopRef.current}
            onFirstPaint={() => { restoreScrollTopRef.current = null; fadeIn(); }}
            onScrollTopChange={(value) => { scrollTopRef.current = value; }}
          />
        ) : <ManagerSurface nodeId={displayedNodeId} />}
      </div>
    </div>
  );
}
