import type { LayoutItem } from '../layoutMath';
import { BUCKET_SIZE } from '../layoutMath';
import type { CanvasScrollDirection, CanvasScrollPhase } from '../../../shared/lib/canvas/scrollState';

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
  bucketIndex?: Map<number, number[]> | null;
}): CanvasVisibilityPlan {
  const { positions, scrollTop, viewportHeight, scrollPhase, scrollDirection, queueDepth, bucketIndex } = args;

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

  // Binary search for visible range, then extend by at least 1 row in each
  // direction so adjacent tiles are always ready — prevents flicker when
  // images are large relative to the viewport.
  const rawStart = lowerBound(positions, top, (p) => p.y + p.h);
  const rawEnd = lowerBound(positions, bottom, (p) => p.y);
  const startIdx = Math.max(0, rawStart - 1);
  const endIdx = Math.min(positions.length, rawEnd + 1);
  const visibleIterEnd = Math.max(0, endIdx - startIdx);

  const prefetchPx = scrollPhase === 'idle' ? PREFETCH_PX : scrollPhase === 'slow' ? 600 : 0;
  const prefetchLimit = getPrimaryPrefetchLimit(scrollPhase, queueDepth);

  const prefetchTop = Math.max(0, scrollTop - prefetchPx);
  const prefetchBottom = scrollTop + viewportHeight + prefetchPx;

  if (bucketIndex && bucketIndex.size > 0) {
    // Expand the visible window by one bucket so adjacent rows are always loaded
    const visibleIndices = collectBucketWindowIndices(positions, bucketIndex, top - BUCKET_SIZE, bottom + BUCKET_SIZE);
    const visibleSet = new Set(visibleIndices);
    const prefetchCandidates = collectBucketWindowIndices(positions, bucketIndex, prefetchTop, prefetchBottom)
      .filter((index) => !visibleSet.has(index));
    const below: number[] = [];
    const above: number[] = [];
    for (const index of prefetchCandidates) {
      const pos = positions[index];
      if (!pos) continue;
      if (pos.y >= bottom) below.push(index);
      else above.push(index);
    }
    const prefetchIndices = buildDirectionalPrefetch({
      forward: below,
      backward: above.reverse(),
      scrollPhase,
      scrollDirection,
      primaryLimit: prefetchLimit,
    });
    const { cancelTop, cancelBottom } = buildCancelWindow({
      scrollTop,
      viewportHeight,
      scrollPhase,
      scrollDirection,
    });
    return {
      startIdx: 0,
      endIdx: visibleIndices.length,
      visibleIndices,
      visibleIterEnd: visibleIndices.length,
      prefetchIndices,
      cancelTop,
      cancelBottom,
    };
  }

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
  const prefetchIndices = buildDirectionalPrefetch({
    forward,
    backward,
    scrollPhase,
    scrollDirection,
    primaryLimit: prefetchLimit,
  });
  const { cancelTop, cancelBottom } = buildCancelWindow({
    scrollTop,
    viewportHeight,
    scrollPhase,
    scrollDirection,
  });
  return {
    startIdx,
    endIdx,
    visibleIndices: null,
    visibleIterEnd,
    prefetchIndices,
    cancelTop,
    cancelBottom,
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

// Pre-allocated scratch buffers reused across frames to avoid GC pressure.
const scratchSeen = new Set<number>();
const scratchIndices: number[] = [];

function collectBucketWindowIndices(
  positions: LayoutItem[],
  bucketIndex: Map<number, number[]>,
  top: number,
  bottom: number,
): number[] {
  const startBucket = Math.floor(top / BUCKET_SIZE);
  const endBucket = Math.floor(bottom / BUCKET_SIZE);
  scratchSeen.clear();
  scratchIndices.length = 0;

  for (let bucket = startBucket; bucket <= endBucket; bucket += 1) {
    const bucketIndices = bucketIndex.get(bucket);
    if (!bucketIndices) continue;
    for (const index of bucketIndices) {
      if (scratchSeen.has(index)) continue;
      scratchSeen.add(index);
      const pos = positions[index];
      if (!pos) continue;
      if (pos.y + pos.h < top || pos.y > bottom) continue;
      scratchIndices.push(index);
    }
  }

  scratchIndices.sort((a, b) => {
    const posA = positions[a];
    const posB = positions[b];
    if (posA.y !== posB.y) return posA.y - posB.y;
    if (posA.x !== posB.x) return posA.x - posB.x;
    return a - b;
  });
  // Return a snapshot — caller may hold reference across calls
  return scratchIndices.slice();
}
