import { useCallback, useRef } from 'react';
import { computeLayout, computeTextHeight } from '../VirtualGrid';
import { prefetchMetadata } from '../metadataPrefetch';
import type { MasonryImageItem } from '../shared';
import type { GridRuntimeAction, GridRuntimeState, GridViewMode } from '../runtime';

interface UseGridKeyboardNavigationArgs {
  stateRef: { current: GridRuntimeState };
  imagesRef: { current: MasonryImageItem[] };
  lastClickedHashRef: { current: string | null };
  displayViewModeRef: { current: GridViewMode };
  displaySettingsRef: { current: { showTileName: boolean; showResolution: boolean } };
  gap: number;
  dispatch: React.Dispatch<GridRuntimeAction>;
  onContainerWidthChange?: (width: number) => void;
}

interface GridKeyboardNavigationResult {
  scrollRef: React.RefObject<HTMLDivElement>;
  getCanvasOffsetTop: () => number;
  handleContainerWidthChange: (width: number) => void;
  scrollToIndex: (index: number) => void;
  handleGridNavigation: (key: string, shiftKey: boolean) => void;
}

export function useGridKeyboardNavigation({
  stateRef,
  imagesRef,
  lastClickedHashRef,
  displayViewModeRef,
  displaySettingsRef,
  gap,
  dispatch,
  onContainerWidthChange,
}: UseGridKeyboardNavigationArgs): GridKeyboardNavigationResult {
  const containerWidthRef = useRef(0);
  const targetSizeRef = useRef(stateRef.current.displayTargetSize);
  targetSizeRef.current = stateRef.current.displayTargetSize;
  const shiftAnchorIndexRef = useRef<number | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  const getCanvasOffsetTop = useCallback((): number => {
    const container = scrollRef.current;
    if (!container) return 0;
    const canvasRoot = container.querySelector<HTMLElement>('[data-canvas-grid-root]');
    if (!canvasRoot) return 0;
    const scrollRect = container.getBoundingClientRect();
    const canvasRect = canvasRoot.getBoundingClientRect();
    return container.scrollTop + (canvasRect.top - scrollRect.top);
  }, []);

  const handleContainerWidthChange = useCallback(
    (width: number) => {
      containerWidthRef.current = width;
      onContainerWidthChange?.(width);
    },
    [onContainerWidthChange],
  );

  const scrollToIndex = useCallback(
    (index: number) => {
      requestAnimationFrame(() => {
        const scrollEl = scrollRef.current;
        if (!scrollEl) return;
        const currentImages = imagesRef.current;
        if (index < 0 || index >= currentImages.length) return;
        const hash = currentImages[index].hash;
        const tile = scrollEl.querySelector(`[data-hash="${hash}"]`);
        if (tile) {
          (tile as HTMLElement).scrollIntoView({ block: 'nearest' });
          return;
        }
        const cw = containerWidthRef.current;
        const ts = targetSizeRef.current;
        const vm = displayViewModeRef.current;
        const ds = displaySettingsRef.current;
        const textHeight = computeTextHeight(ds.showTileName, ds.showResolution);
        const layout = computeLayout(currentImages, cw, ts, gap, vm, textHeight);
        const pos = layout.positions[index];
        if (!pos) return;
        const viewportH = scrollEl.clientHeight;
        const canvasOffsetTop = getCanvasOffsetTop();
        const localScrollTop = Math.max(0, scrollEl.scrollTop - canvasOffsetTop);
        if (pos.y < localScrollTop) {
          scrollEl.scrollTop = Math.max(0, canvasOffsetTop + pos.y);
        } else if (pos.y + pos.h > localScrollTop + viewportH) {
          scrollEl.scrollTop = Math.max(0, canvasOffsetTop + pos.y + pos.h - viewportH);
        }
      });
    },
    [displaySettingsRef, displayViewModeRef, gap, getCanvasOffsetTop, imagesRef],
  );

  const handleGridNavigation = useCallback(
    (key: string, shiftKey: boolean) => {
      const currentImages = imagesRef.current;
      if (currentImages.length === 0) return;

      const cw = containerWidthRef.current;
      const ts = targetSizeRef.current;
      const g = gap;
      const columnCount = Math.max(1, Math.round((cw + g) / (ts + g)));

      let currentIndex = -1;
      const lastHash = lastClickedHashRef.current;
      if (lastHash) {
        currentIndex = currentImages.findIndex((i) => i.hash === lastHash);
      }
      if (currentIndex === -1 && currentImages.length > 0) {
        if (shiftKey && stateRef.current.selectedHashes.size > 0) {
          for (let i = 0; i < currentImages.length; i++) {
            if (stateRef.current.selectedHashes.has(currentImages[i].hash)) {
              currentIndex = i;
              break;
            }
          }
        }
        if (currentIndex === -1) currentIndex = 0;
        const hash = currentImages[currentIndex].hash;
        dispatch({ type: 'SELECT_HASHES', hashes: new Set([hash]) });
        dispatch({ type: 'SET_LAST_CLICKED', hash });
        dispatch({ type: 'DEACTIVATE_VIRTUAL_SELECT_ALL' });
        shiftAnchorIndexRef.current = null;
        prefetchMetadata(hash);
        scrollToIndex(currentIndex);
        return;
      }

      const scrollEl = scrollRef.current;
      const viewportH = scrollEl ? scrollEl.clientHeight : 500;
      const ds = displaySettingsRef.current;
      const textHeight = computeTextHeight(ds.showTileName, ds.showResolution);
      const cellH = ts + textHeight + g;
      const visibleRows = Math.max(1, Math.floor(viewportH / cellH));

      let targetIndex: number;
      switch (key) {
        case 'ArrowLeft':
          targetIndex = currentIndex - 1;
          break;
        case 'ArrowRight':
          targetIndex = currentIndex + 1;
          break;
        case 'ArrowUp':
          targetIndex = currentIndex - columnCount;
          break;
        case 'ArrowDown':
          targetIndex = currentIndex + columnCount;
          break;
        case 'Home':
          targetIndex = 0;
          break;
        case 'End':
          targetIndex = currentImages.length - 1;
          break;
        case 'PageUp':
          targetIndex = currentIndex - columnCount * visibleRows;
          break;
        case 'PageDown':
          targetIndex = currentIndex + columnCount * visibleRows;
          break;
        default:
          return;
      }

      targetIndex = Math.max(0, Math.min(currentImages.length - 1, targetIndex));
      if (targetIndex === currentIndex) return;

      const targetHash = currentImages[targetIndex].hash;
      if (shiftKey) {
        if (shiftAnchorIndexRef.current === null) {
          shiftAnchorIndexRef.current = currentIndex;
        }
        const anchor = shiftAnchorIndexRef.current;
        const lo = Math.min(anchor, targetIndex);
        const hi = Math.max(anchor, targetIndex);
        const rangeHashes: string[] = [];
        for (let i = lo; i <= hi; i++) {
          rangeHashes.push(currentImages[i].hash);
        }
        dispatch({ type: 'SELECT_HASHES', hashes: new Set(rangeHashes) });
      } else {
        shiftAnchorIndexRef.current = null;
        dispatch({ type: 'SELECT_HASHES', hashes: new Set([targetHash]) });
      }

      dispatch({ type: 'SET_LAST_CLICKED', hash: targetHash });
      dispatch({ type: 'DEACTIVATE_VIRTUAL_SELECT_ALL' });
      prefetchMetadata(targetHash);
      scrollToIndex(targetIndex);
    },
    [dispatch, displaySettingsRef, gap, imagesRef, lastClickedHashRef, scrollToIndex, stateRef],
  );

  return {
    scrollRef,
    getCanvasOffsetTop,
    handleContainerWidthChange,
    scrollToIndex,
    handleGridNavigation,
  };
}
