import type { LayoutItem } from '../layout/types';
import type { CanvasScrollDirection, CanvasScrollPhase } from './scrollState';

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

const PREFETCH_PX = 800;
const FAST_PRIMARY_PREFETCH_LIMIT = 0;
const SLOW_PRIMARY_PREFETCH_LIMIT = 6;
const IDLE_PRIMARY_PREFETCH_LIMIT = 12;
const IDLE_BACKFILL_LIMIT = 4;

export function buildCanvasVisibilityPlan(args: {
  positions: LayoutItem[];
  scrollTop: number;
  viewportHeight: number;
  scrollPhase: CanvasScrollPhase;
  scrollDirection: CanvasScrollDirection;
  queueDepth: number;
}): CanvasVisibilityPlan {
  const { positions, scrollTop, viewportHeight, scrollPhase, scrollDirection, queueDepth } = args;

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
  const rawStart = lowerBound(positions, top, (p) => p.y + p.h);
  const rawEnd = lowerBound(positions, bottom, (p) => p.y);
  // Widen the range to account for waterfall layouts where Y positions
  // are not monotonically sorted by index. The drawBase loop has its own
  // per-tile bounds check (drawY + pos.h < 0 || drawY > cssH) that skips
  // tiles actually off-screen, so over-including is safe.
  const COL_MARGIN = 20; // extra indices each side to cover column stagger
  const startIdx = Math.max(0, rawStart - COL_MARGIN);
  const endIdx = Math.min(positions.length, rawEnd + COL_MARGIN);
  const visibleIterEnd = Math.max(0, endIdx - startIdx);

  const prefetchPx = scrollPhase === 'idle' ? PREFETCH_PX : scrollPhase === 'slow' ? 600 : 0;
  const prefetchLimit = getPrimaryPrefetchLimit(scrollPhase, queueDepth);
  const prefetchTop = Math.max(0, scrollTop - prefetchPx);
  const prefetchBottom = scrollTop + viewportHeight + prefetchPx;
  const pfStart = lowerBound(positions, prefetchTop, (p) => p.y + p.h);
  const pfEnd = lowerBound(positions, prefetchBottom, (p) => p.y);

  const forward: number[] = [];
  const backward: number[] = [];
  for (let i = endIdx; i < pfEnd && i < positions.length; i++) {
    forward.push(i);
  }
  for (let i = startIdx - 1; i >= pfStart && i >= 0; i--) {
    backward.push(i);
  }

  return {
    startIdx,
    endIdx,
    visibleIndices: null,
    visibleIterEnd,
    prefetchIndices: buildDirectionalPrefetch({
      forward,
      backward,
      scrollPhase,
      scrollDirection,
      primaryLimit: prefetchLimit,
    }),
    ...buildCancelWindow({ scrollTop, viewportHeight, scrollPhase, scrollDirection }),
  };
}

function getPrimaryPrefetchLimit(scrollPhase: CanvasScrollPhase, queueDepth: number): number {
  if (scrollPhase === 'fast') return FAST_PRIMARY_PREFETCH_LIMIT;
  if (scrollPhase === 'slow') return SLOW_PRIMARY_PREFETCH_LIMIT;

  let limit = IDLE_PRIMARY_PREFETCH_LIMIT;
  if (queueDepth > 240) limit = Math.min(limit, 8);
  else if (queueDepth > 160) limit = Math.min(limit, 12);
  else if (queueDepth > 100) limit = Math.min(limit, 16);
  return limit;
}

function buildDirectionalPrefetch(args: {
  forward: number[];
  backward: number[];
  scrollPhase: CanvasScrollPhase;
  scrollDirection: CanvasScrollDirection;
  primaryLimit: number;
}): number[] {
  const { forward, backward, scrollPhase, scrollDirection, primaryLimit } = args;
  if (primaryLimit <= 0) return [];

  if (scrollPhase === 'slow') {
    if (scrollDirection === 'backward') return backward.slice(0, primaryLimit);
    if (scrollDirection === 'forward') return forward.slice(0, primaryLimit);
    return [];
  }

  if (scrollDirection === 'backward') {
    return backward
      .slice(0, primaryLimit)
      .concat(forward.slice(0, Math.min(IDLE_BACKFILL_LIMIT, Math.max(0, primaryLimit - backward.length))));
  }

  return forward
    .slice(0, primaryLimit)
    .concat(backward.slice(0, Math.min(IDLE_BACKFILL_LIMIT, Math.max(0, primaryLimit - forward.length))));
}

function buildCancelWindow(args: {
  scrollTop: number;
  viewportHeight: number;
  scrollPhase: CanvasScrollPhase;
  scrollDirection: CanvasScrollDirection;
}) {
  const { scrollTop, viewportHeight, scrollPhase, scrollDirection } = args;
  let behindMultiplier = 2;
  let aheadMultiplier = 3;

  if (scrollPhase === 'fast') {
    behindMultiplier = 0.5;
    aheadMultiplier = 1;
  } else if (scrollPhase === 'slow') {
    behindMultiplier = 0.75;
    aheadMultiplier = 1.5;
  } else {
    behindMultiplier = 1;
    aheadMultiplier = 1.5;
  }

  if (scrollDirection === 'backward') {
    [behindMultiplier, aheadMultiplier] = [aheadMultiplier, behindMultiplier];
  } else if (scrollDirection === 'unknown') {
    const symmetric = Math.max(behindMultiplier, aheadMultiplier);
    behindMultiplier = symmetric;
    aheadMultiplier = symmetric;
  }

  return {
    cancelTop: scrollTop - viewportHeight * behindMultiplier,
    cancelBottom: scrollTop + viewportHeight + viewportHeight * aheadMultiplier,
  };
}
