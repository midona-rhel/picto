import type { GridViewMode } from '../runtime';
import { BUCKET_SIZE, type LayoutItem } from '../layoutMath';

export interface WaterfallSeenState {
  seen: Uint32Array;
  token: number;
}

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

export function getVisibleIndexRange(
  positions: LayoutItem[],
  scrollTop: number,
  viewportHeight: number,
  mode: GridViewMode,
  bucketIndex: Map<number, number[]> | null,
): [number, number] {
  if (positions.length === 0 || viewportHeight === 0) return [0, 0];

  const top = scrollTop;
  const bottom = scrollTop + viewportHeight;

  if (mode !== 'waterfall') {
    const start = lowerBound(positions, top, (p) => p.y + p.h);
    const end = lowerBound(positions, bottom, (p) => p.y);
    return [start, end];
  }

  if (bucketIndex) {
    const startBucket = Math.floor(top / BUCKET_SIZE);
    const endBucket = Math.floor(bottom / BUCKET_SIZE);
    let minIdx = positions.length;
    let maxIdx = 0;
    for (let b = startBucket; b <= endBucket; b++) {
      const indices = bucketIndex.get(b);
      if (!indices) continue;
      for (const idx of indices) {
        const pos = positions[idx];
        if (pos.y + pos.h > top && pos.y < bottom) {
          if (idx < minIdx) minIdx = idx;
          if (idx > maxIdx) maxIdx = idx;
        }
      }
    }
    return minIdx <= maxIdx ? [minIdx, maxIdx + 1] : [0, 0];
  }

  let start = positions.length;
  let end = 0;
  for (let i = 0; i < positions.length; i++) {
    const pos = positions[i];
    if (pos.y + pos.h > top && pos.y < bottom) {
      if (i < start) start = i;
      if (i >= end) end = i + 1;
    }
  }
  return start <= end ? [start, end] : [0, 0];
}

export function collectWaterfallIndices(
  positions: LayoutItem[],
  top: number,
  bottom: number,
  bucketIndex: Map<number, number[]> | null,
  seenState: WaterfallSeenState,
  out: number[],
): number[] {
  out.length = 0;
  if (positions.length === 0 || bottom <= top || !bucketIndex) return out;

  if (seenState.seen.length < positions.length) {
    seenState.seen = new Uint32Array(positions.length);
    seenState.token = 1;
  }

  let token = seenState.token + 1;
  if (token >= 0x7fffffff) {
    seenState.seen.fill(0);
    token = 1;
  }
  seenState.token = token;

  const startBucket = Math.floor(top / BUCKET_SIZE);
  const endBucket = Math.floor(bottom / BUCKET_SIZE);
  for (let b = startBucket; b <= endBucket; b++) {
    const indices = bucketIndex.get(b);
    if (!indices) continue;
    for (let k = 0; k < indices.length; k++) {
      const idx = indices[k];
      if (seenState.seen[idx] === token) continue;
      const pos = positions[idx];
      if (!pos) continue;
      if (pos.y + pos.h <= top || pos.y >= bottom) continue;
      seenState.seen[idx] = token;
      out.push(idx);
    }
  }

  return out;
}

export function buildCanvasVisibilityPlan(args: {
  positions: LayoutItem[];
  scrollTop: number;
  viewportHeight: number;
  mode: GridViewMode;
  bucketIndex: Map<number, number[]> | null;
  isScrolling: boolean;
  queueDepth: number;
  waterfallVisibleOut: number[];
  waterfallPrefetchOut: number[];
  waterfallSeenState: WaterfallSeenState;
}): CanvasVisibilityPlan {
  const {
    positions,
    scrollTop,
    viewportHeight,
    mode,
    bucketIndex,
    isScrolling,
    queueDepth,
    waterfallVisibleOut,
    waterfallPrefetchOut,
    waterfallSeenState,
  } = args;
  const isWaterfall = mode === 'waterfall';
  const [startIdx, endIdx] = getVisibleIndexRange(
    positions,
    scrollTop,
    viewportHeight,
    mode,
    bucketIndex,
  );
  const visibleIndices = isWaterfall
    ? collectWaterfallIndices(
        positions,
        scrollTop,
        scrollTop + viewportHeight,
        bucketIndex,
        waterfallSeenState,
        waterfallVisibleOut,
      )
    : null;
  const visibleIterEnd = visibleIndices
    ? visibleIndices.length
    : Math.max(0, Math.min(endIdx, positions.length) - startIdx);

  const prefetchPx = isScrolling ? 900 : 3200;
  let prefetchLimit = isScrolling ? 28 : 420;
  if (queueDepth > 240) prefetchLimit = Math.min(prefetchLimit, 64);
  else if (queueDepth > 160) prefetchLimit = Math.min(prefetchLimit, 96);
  else if (queueDepth > 100) prefetchLimit = Math.min(prefetchLimit, 140);

  const prefetchTop = scrollTop - prefetchPx;
  const prefetchBottom = scrollTop + viewportHeight + prefetchPx;
  const prefetchIndices: number[] = [];

  if (isWaterfall) {
    const nearby = collectWaterfallIndices(
      positions,
      Math.max(0, prefetchTop),
      prefetchBottom,
      bucketIndex,
      waterfallSeenState,
      waterfallPrefetchOut,
    );
    for (let n = 0; n < nearby.length && prefetchIndices.length < prefetchLimit; n++) {
      const i = nearby[n];
      const pos = positions[i];
      if (pos.y + pos.h > scrollTop && pos.y < scrollTop + viewportHeight) continue;
      prefetchIndices.push(i);
    }
  } else {
    const [pfStart, pfEnd] = getVisibleIndexRange(
      positions,
      Math.max(0, prefetchTop),
      prefetchBottom - Math.max(0, prefetchTop),
      mode,
      bucketIndex,
    );
    for (let i = endIdx; i < pfEnd && i < positions.length && prefetchIndices.length < prefetchLimit; i++) {
      if (i >= startIdx && i < endIdx) continue;
      prefetchIndices.push(i);
    }
    for (let i = startIdx - 1; i >= pfStart && i >= 0 && prefetchIndices.length < prefetchLimit; i--) {
      if (i >= startIdx && i < endIdx) continue;
      prefetchIndices.push(i);
    }
  }

  const cancelPadPx = isScrolling ? 1400 : 2600;
  return {
    startIdx,
    endIdx,
    visibleIndices,
    visibleIterEnd,
    prefetchIndices,
    cancelTop: prefetchTop - cancelPadPx,
    cancelBottom: prefetchBottom + cancelPadPx,
  };
}
