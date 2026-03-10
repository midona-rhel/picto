import type { LayoutItem } from '../layoutMath';

export interface CanvasVisibilityPlan {
  startIdx: number;
  endIdx: number;
  visibleIndices: number[] | null;
  visibleIterEnd: number;
  prefetchIndices: number[];
  cancelTop: number;
  cancelBottom: number;
}

function lowerBound(
  positions: LayoutItem[],
  target: number,
  selector: (item: LayoutItem) => number,
): number {
  let lo = 0;
  let hi = positions.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (selector(positions[mid]) < target) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}

const PREFETCH_PX = 1600;

export function buildCanvasVisibilityPlan(args: {
  positions: LayoutItem[];
  scrollTop: number;
  viewportHeight: number;
  isScrolling: boolean;
  queueDepth: number;
}): CanvasVisibilityPlan {
  const { positions, scrollTop, viewportHeight, isScrolling, queueDepth } = args;

  if (positions.length === 0 || viewportHeight === 0) {
    return {
      startIdx: 0,
      endIdx: 0,
      visibleIndices: null,
      visibleIterEnd: 0,
      prefetchIndices: [],
      cancelTop: scrollTop - 4000,
      cancelBottom: scrollTop + viewportHeight + 4000,
    };
  }

  const top = scrollTop;
  const bottom = scrollTop + viewportHeight;

  // Binary search for visible range
  const startIdx = lowerBound(positions, top, (p) => p.y + p.h);
  const endIdx = lowerBound(positions, bottom, (p) => p.y);
  const visibleIterEnd = Math.max(0, Math.min(endIdx, positions.length) - startIdx);

  // Prefetch window
  const prefetchPx = isScrolling ? 900 : PREFETCH_PX;
  let prefetchLimit = isScrolling ? 28 : 420;
  if (queueDepth > 240) prefetchLimit = Math.min(prefetchLimit, 64);
  else if (queueDepth > 160) prefetchLimit = Math.min(prefetchLimit, 96);
  else if (queueDepth > 100) prefetchLimit = Math.min(prefetchLimit, 140);

  const prefetchTop = Math.max(0, scrollTop - prefetchPx);
  const prefetchBottom = scrollTop + viewportHeight + prefetchPx;

  const pfStart = lowerBound(positions, prefetchTop, (p) => p.y + p.h);
  const pfEnd = lowerBound(positions, prefetchBottom, (p) => p.y);

  const prefetchIndices: number[] = [];
  // Below visible range first (more likely scroll direction)
  for (let i = endIdx; i < pfEnd && i < positions.length && prefetchIndices.length < prefetchLimit; i++) {
    prefetchIndices.push(i);
  }
  // Above visible range
  for (let i = startIdx - 1; i >= pfStart && i >= 0 && prefetchIndices.length < prefetchLimit; i--) {
    prefetchIndices.push(i);
  }

  const cancelPadPx = isScrolling ? 1400 : 2600;
  return {
    startIdx,
    endIdx,
    visibleIndices: null,
    visibleIterEnd,
    prefetchIndices,
    cancelTop: prefetchTop - cancelPadPx,
    cancelBottom: prefetchBottom + cancelPadPx,
  };
}
