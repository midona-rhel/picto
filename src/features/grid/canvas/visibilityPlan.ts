/**
 * Visibility planning — determine which items are visible and should be prefetched.
 * Uses binary search for fast visible-range lookup in large item sets.
 */

import type { LayoutItem } from '../layout/types';
import { lowerBound } from '../layout/layoutMath';

export interface VisibilityPlan {
  /** First visible item index. */
  start: number;
  /** One past the last visible item index. */
  end: number;
  /** Indices ahead of the current scroll direction. */
  aheadPrefetchIndices: number[];
  /** Indices behind the current scroll direction. */
  behindPrefetchIndices: number[];
}

export function buildVisibilityPlan(
  positions: LayoutItem[],
  scrollTop: number,
  viewportHeight: number,
  scrollDirection: -1 | 0 | 1,
): VisibilityPlan {
  if (positions.length === 0) {
    return { start: 0, end: 0, aheadPrefetchIndices: [], behindPrefetchIndices: [] };
  }

  const visibleTop = scrollTop;
  const visibleBottom = scrollTop + viewportHeight;

  // Binary search for first item whose bottom edge is >= visibleTop
  const start = lowerBound(positions, visibleTop);
  // Find last item whose top edge is <= visibleBottom
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

  const aheadPrefetchIndices = collectIndices(positions, aheadTop, aheadBottom);
  const behindPrefetchIndices = collectIndices(positions, behindTop, behindBottom);

  return { start, end, aheadPrefetchIndices, behindPrefetchIndices };
}

function collectIndices(positions: LayoutItem[], fromY: number, toY: number): number[] {
  if (toY <= fromY) return [];
  const start = Math.max(0, lowerBound(positions, fromY));
  const indices: number[] = [];
  for (let i = start; i < positions.length && positions[i].y < toY; i++) {
    indices.push(i);
  }
  return indices;
}
