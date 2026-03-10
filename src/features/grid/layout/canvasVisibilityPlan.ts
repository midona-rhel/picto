import type { LayoutItem } from '../layoutMath';
import { BUCKET_SIZE } from '../layoutMath';

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
  bucketIndex?: Map<number, number[]> | null;
}): CanvasVisibilityPlan {
  const { positions, scrollTop, viewportHeight, isScrolling, queueDepth, bucketIndex } = args;

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

  if (bucketIndex && bucketIndex.size > 0) {
    const visibleIndices = collectBucketWindowIndices(positions, bucketIndex, top, bottom);
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
    const prefetchIndices = below.concat(above.reverse()).slice(0, prefetchLimit);
    const cancelPadPx = isScrolling ? 1400 : 2600;
    return {
      startIdx: 0,
      endIdx: visibleIndices.length,
      visibleIndices,
      visibleIterEnd: visibleIndices.length,
      prefetchIndices,
      cancelTop: prefetchTop - cancelPadPx,
      cancelBottom: prefetchBottom + cancelPadPx,
    };
  }

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

function collectBucketWindowIndices(
  positions: LayoutItem[],
  bucketIndex: Map<number, number[]>,
  top: number,
  bottom: number,
): number[] {
  const startBucket = Math.floor(top / BUCKET_SIZE);
  const endBucket = Math.floor(bottom / BUCKET_SIZE);
  const seen = new Set<number>();
  const indices: number[] = [];

  for (let bucket = startBucket; bucket <= endBucket; bucket += 1) {
    const bucketIndices = bucketIndex.get(bucket);
    if (!bucketIndices) continue;
    for (const index of bucketIndices) {
      if (seen.has(index)) continue;
      seen.add(index);
      const pos = positions[index];
      if (!pos) continue;
      if (pos.y + pos.h < top || pos.y > bottom) continue;
      indices.push(index);
    }
  }

  indices.sort((a, b) => {
    const posA = positions[a];
    const posB = positions[b];
    if (posA.y !== posB.y) return posA.y - posB.y;
    if (posA.x !== posB.x) return posA.x - posB.x;
    return a - b;
  });
  return indices;
}
