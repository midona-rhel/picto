import { useEffect, type RefObject } from 'react';
import type { LayoutItem } from '../layoutMath';

export function useCanvasScrollAnchor(args: {
  layout: { positions: LayoutItem[] };
  prevLayoutRef: { current: { positions: LayoutItem[] } };
  scrollContainerRef?: RefObject<HTMLDivElement | null>;
  getScrollMetrics: () => { localScrollTop: number; canvasTopInScroll: number; viewportHeight: number };
}) {
  const { layout, prevLayoutRef, scrollContainerRef, getScrollMetrics } = args;

  useEffect(() => {
    const prev = prevLayoutRef.current;
    prevLayoutRef.current = layout;
    if (!prev || prev.positions === layout.positions) return;
    if (prev.positions.length !== layout.positions.length) return;

    const scrollEl = scrollContainerRef?.current;
    if (!scrollEl) return;
    const metrics = getScrollMetrics();
    const st = metrics.localScrollTop;
    const vh = metrics.viewportHeight;
    if (vh === 0) return;

    const viewportCenter = st + vh / 2;
    let anchorIdx = -1;
    let bestDist = Infinity;
    for (let i = 0; i < prev.positions.length; i++) {
      const p = prev.positions[i];
      const tileCenter = p.y + p.h / 2;
      const dist = Math.abs(tileCenter - viewportCenter);
      if (dist < bestDist) {
        bestDist = dist;
        anchorIdx = i;
      }
    }
    if (anchorIdx < 0 || anchorIdx >= layout.positions.length) return;

    const oldTileCenter = prev.positions[anchorIdx].y + prev.positions[anchorIdx].h / 2;
    const offsetInViewport = oldTileCenter - st;
    const newTileCenter = layout.positions[anchorIdx].y + layout.positions[anchorIdx].h / 2;
    const newScrollTop = newTileCenter - offsetInViewport;
    scrollEl.scrollTop = Math.max(0, metrics.canvasTopInScroll + newScrollTop);
  }, [getScrollMetrics, layout, prevLayoutRef, scrollContainerRef]);
}
