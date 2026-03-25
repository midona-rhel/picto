/**
 * Visibility planning — determine which items are visible and should be prefetched.
 * Uses binary search for fast visible-range lookup in large item sets.
 *
 * Accepts scratch arrays for prefetch indices to avoid per-frame allocations.
 */

import type { LayoutItem } from '../layout/types';
import { lowerBound } from '../layout/layoutMath';

export interface VisibilityPlan {
  start: number;
  end: number;
  aheadPrefetchIndices: number[];
  behindPrefetchIndices: number[];
}

export function buildVisibilityPlan(
  positions: LayoutItem[],
  scrollTop: number,
  viewportHeight: number,
  scrollDirection: -1 | 0 | 1,
  aheadOut: number[] = [],
  behindOut: number[] = [],
): VisibilityPlan {
  aheadOut.length = 0;
  behindOut.length = 0;

  if (positions.length === 0) {
    return { start: 0, end: 0, aheadPrefetchIndices: aheadOut, behindPrefetchIndices: behindOut };
  }

  // Extend visible range by ~1 row above and below to prevent flicker.
  // Estimate row height from the first position (all rows are similar height).
  const rowH = positions[0].h;
  const visibleTop = Math.max(0, scrollTop - rowH);
  const visibleBottom = scrollTop + viewportHeight + rowH;

  const start = lowerBound(positions, visibleTop);
  let end = start;
  while (end < positions.length && positions[end].y < visibleBottom) {
    end++;
  }

  const forwardDistance = viewportHeight * 0.75;
  const backwardDistance = viewportHeight * 0.25;

  const preferDown = scrollDirection >= 0;
  const aheadTop = preferDown ? visibleBottom : Math.max(0, scrollTop - forwardDistance);
  const aheadBottom = preferDown
    ? scrollTop + viewportHeight + forwardDistance
    : visibleTop;
  const behindTop = preferDown ? Math.max(0, scrollTop - backwardDistance) : visibleBottom;
  const behindBottom = preferDown
    ? visibleTop
    : scrollTop + viewportHeight + backwardDistance;

  collectInto(positions, aheadTop, aheadBottom, aheadOut);
  collectInto(positions, behindTop, behindBottom, behindOut);

  return { start, end, aheadPrefetchIndices: aheadOut, behindPrefetchIndices: behindOut };
}

function collectInto(positions: LayoutItem[], fromY: number, toY: number, out: number[]) {
  if (toY <= fromY) return;
  const start = Math.max(0, lowerBound(positions, fromY));
  for (let i = start; i < positions.length && positions[i].y < toY; i++) {
    out.push(i);
  }
}
