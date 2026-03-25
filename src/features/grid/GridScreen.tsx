/**
 * Grid screen — feature root. Reads state, delegates to CanvasGrid.
 */

import { useEffect, useRef, useState, useCallback } from 'react';
import { useAtomValue } from 'jotai';
import { IconPhotoOff } from '@tabler/icons-react';
import { activeNodeIdAtom } from '../../state/navigation';
import {
  gridItemsAtom,
  gridLoadingAtom,
  gridErrorAtom,
  gridCursorAtom,
  gridViewModeAtom,
  gridTargetSizeAtom,
  gridShowNameAtom,
  gridShowExtensionAtom,
} from '../../state/grid';
import { gridController } from '../../controllers/gridController';
import { CanvasGrid } from './canvas/CanvasGrid';
import type { BaseScope, CanonicalEntityGridItem } from '../../shared/types/canonical';
import type { GridViewMode } from './layout/types';
import styles from './GridScreen.module.css';

const GRID_SYSTEM_SCOPES: Record<string, string> = {
  'system:active': 'all',
  'system:inbox': 'inbox',
  'system:trash': 'trash',
  'system:uncategorized': 'uncategorized',
  'system:untagged': 'untagged',
};

const NON_GRID_NODES = new Set(['system:duplicates', 'system:recent_viewed']);
const SCOPE_TRANSITION_MS = 250;

interface SurfaceSnapshot {
  items: CanonicalEntityGridItem[];
  viewMode: GridViewMode;
  targetSize: number;
  showName: boolean;
  showExtension: boolean;
  scrollTop: number;
}

function nodeIdToScope(nodeId: string): BaseScope | null {
  if (nodeId.startsWith('folder:')) {
    const id = parseInt(nodeId.slice(7), 10);
    return { kind: 'folder', id: isNaN(id) ? 0 : id };
  }
  if (nodeId.startsWith('smart:')) {
    const id = parseInt(nodeId.slice(6), 10);
    return { kind: 'smart_folder', id: isNaN(id) ? 0 : id };
  }
  if (NON_GRID_NODES.has(nodeId)) return null;
  const scopeKey = GRID_SYSTEM_SCOPES[nodeId];
  if (scopeKey) return { kind: 'system', key: scopeKey };
  return null;
}

export function GridScreen() {
  const activeNodeId = useAtomValue(activeNodeIdAtom);
  const items = useAtomValue(gridItemsAtom);
  const loading = useAtomValue(gridLoadingAtom);
  const error = useAtomValue(gridErrorAtom);
  const cursor = useAtomValue(gridCursorAtom);
  const viewMode = useAtomValue(gridViewModeAtom);
  const targetSize = useAtomValue(gridTargetSizeAtom);
  const showName = useAtomValue(gridShowNameAtom);
  const showExtension = useAtomValue(gridShowExtensionAtom);

  const [outgoingSurface, setOutgoingSurface] = useState<SurfaceSnapshot | null>(null);
  const [transitionPhase, setTransitionPhase] = useState<'idle' | 'fading_out' | 'waiting' | 'fading_in'>('idle');
  const lastScrollTopRef = useRef(0);
  const previousNodeIdRef = useRef(activeNodeId);
  const transitionTimerRef = useRef<number | null>(null);
  const fadeInFrameRef = useRef<number | null>(null);
  const latestRenderableSurfaceRef = useRef<SurfaceSnapshot | null>(null);
  const pendingNodeIdRef = useRef(activeNodeId);

  const scope = nodeIdToScope(activeNodeId);
  const isGridScope = scope !== null;

  const clearTransition = useCallback(() => {
    if (transitionTimerRef.current != null) {
      window.clearTimeout(transitionTimerRef.current);
      transitionTimerRef.current = null;
    }
    if (fadeInFrameRef.current != null) {
      window.cancelAnimationFrame(fadeInFrameRef.current);
      fadeInFrameRef.current = null;
    }
    setOutgoingSurface(null);
    setTransitionPhase('idle');
  }, []);

  useEffect(() => {
    if (items.length > 0 && isGridScope) {
      latestRenderableSurfaceRef.current = {
        items,
        viewMode,
        targetSize,
        showName,
        showExtension,
        scrollTop: lastScrollTopRef.current,
      };
    }
  }, [isGridScope, items, showExtension, showName, targetSize, viewMode]);

  const beginFadeIn = useCallback(() => {
    if (fadeInFrameRef.current != null) {
      window.cancelAnimationFrame(fadeInFrameRef.current);
      fadeInFrameRef.current = null;
    }
    fadeInFrameRef.current = window.requestAnimationFrame(() => {
      fadeInFrameRef.current = null;
      setTransitionPhase((phase) => {
        if (phase !== 'waiting') return phase;
        if (transitionTimerRef.current != null) {
          window.clearTimeout(transitionTimerRef.current);
        }
        transitionTimerRef.current = window.setTimeout(() => {
          transitionTimerRef.current = null;
          setTransitionPhase('idle');
        }, SCOPE_TRANSITION_MS);
        return 'fading_in';
      });
    });
  }, []);

  useEffect(() => {
    const previousScope = nodeIdToScope(previousNodeIdRef.current);
    const nextScope = nodeIdToScope(activeNodeId);
    pendingNodeIdRef.current = activeNodeId;

    if (transitionTimerRef.current != null) {
      window.clearTimeout(transitionTimerRef.current);
      transitionTimerRef.current = null;
    }
    if (fadeInFrameRef.current != null) {
      window.cancelAnimationFrame(fadeInFrameRef.current);
      fadeInFrameRef.current = null;
    }

    if (previousScope && nextScope && latestRenderableSurfaceRef.current) {
      setOutgoingSurface(latestRenderableSurfaceRef.current);
      setTransitionPhase('fading_out');
      transitionTimerRef.current = window.setTimeout(() => {
        transitionTimerRef.current = null;
        const committedNodeId = pendingNodeIdRef.current;
        const committedScope = nodeIdToScope(committedNodeId);
        setOutgoingSurface(null);
        setTransitionPhase('waiting');
        if (committedScope) {
          void gridController.navigateTo(committedScope);
        } else {
          gridController.deactivate();
        }
        previousNodeIdRef.current = committedNodeId;
      }, SCOPE_TRANSITION_MS);
      return;
    }

    if (nextScope) {
      void gridController.navigateTo(nextScope);
      previousNodeIdRef.current = activeNodeId;
    } else {
      gridController.deactivate();
      previousNodeIdRef.current = activeNodeId;
      clearTransition();
    }
  }, [activeNodeId]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (transitionPhase !== 'waiting') return;
    if (!loading && (error || items.length === 0 || !isGridScope)) {
      beginFadeIn();
    }
  }, [beginFadeIn, error, isGridScope, items.length, loading, transitionPhase]);

  useEffect(() => () => {
    if (transitionTimerRef.current != null) {
      window.clearTimeout(transitionTimerRef.current);
    }
    if (fadeInFrameRef.current != null) {
      window.cancelAnimationFrame(fadeInFrameRef.current);
    }
  }, []);

  const incomingHidden = transitionPhase === 'fading_out' || transitionPhase === 'waiting';
  const incomingFadingIn = transitionPhase === 'fading_in';
  const outgoingFading = outgoingSurface && transitionPhase === 'fading_out';
  const isEmpty = items.length === 0 && !loading;

  const renderIncomingSurface = () => {
    if (!isGridScope) {
      return <div className={styles.nonGridPlaceholder}>This view is not available yet</div>;
    }

    if (error) {
      return (
        <div className={styles.error}>
          <span>{error}</span>
          <button className={styles.retryBtn} onClick={() => gridController.loadFirstPage()}>
            Retry
          </button>
        </div>
      );
    }

    if (isEmpty) {
      return (
        <div className={styles.empty}>
          <IconPhotoOff size={32} stroke={1} className={styles.emptyIcon} />
          <span>No items</span>
        </div>
      );
    }

    return (
      <CanvasGrid
        items={items}
        viewMode={viewMode}
        targetSize={targetSize}
        showName={showName}
        showExtension={showExtension}
        onFirstPaint={beginFadeIn}
        onScrollTopChange={(scrollTop) => { lastScrollTopRef.current = scrollTop; }}
        onTileClick={(_index, _item) => { /* TODO: selection / viewer — PBI-593 */ }}
        onLoadMore={cursor ? () => gridController.loadNextPage() : undefined}
      />
    );
  };

  return (
    <div className={styles.root}>
      <div className={styles.surfaceStack}>
        <div
          className={`${styles.surface} ${
            incomingHidden
              ? styles.surfaceIncomingHidden
              : incomingFadingIn
                ? styles.surfaceIncomingFadeIn
                : styles.surfaceIncomingVisible
          }`}
        >
          {renderIncomingSurface()}
        </div>

        {outgoingSurface && (
          <div
            className={`${styles.surface} ${styles.surfaceOutgoing} ${
              outgoingFading ? styles.surfaceOutgoingFade : ''
            }`}
          >
            <CanvasGrid
              items={outgoingSurface.items}
              viewMode={outgoingSurface.viewMode}
              targetSize={outgoingSurface.targetSize}
              showName={outgoingSurface.showName}
              showExtension={outgoingSurface.showExtension}
              interactive={false}
              frozenScrollTop={outgoingSurface.scrollTop}
            />
          </div>
        )}
      </div>
    </div>
  );
}
