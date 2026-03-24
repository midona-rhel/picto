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
  /** Indices to prefetch (outside visible but within prefetch zone). */
  prefetchIndices: number[];
}

const PREFETCH_PX = 600;
const EXTEND_ROWS_PX = 100; // Extra pixels to prevent flicker at edges

export function buildVisibilityPlan(
  positions: LayoutItem[],
  scrollTop: number,
  viewportHeight: number,
): VisibilityPlan {
  if (positions.length === 0) {
    return { start: 0, end: 0, prefetchIndices: [] };
  }

  const visibleTop = scrollTop - EXTEND_ROWS_PX;
  const visibleBottom = scrollTop + viewportHeight + EXTEND_ROWS_PX;

  // Binary search for first item whose bottom edge is >= visibleTop
  const start = lowerBound(positions, visibleTop);
  // Find last item whose top edge is <= visibleBottom
  let end = start;
  while (end < positions.length && positions[end].y < visibleBottom) {
    end++;
  }

  // Prefetch zone
  const prefetchTop = scrollTop - PREFETCH_PX;
  const prefetchBottom = scrollTop + viewportHeight + PREFETCH_PX;
  const prefetchStart = Math.max(0, lowerBound(positions, prefetchTop));
  let prefetchEnd = end;
  while (prefetchEnd < positions.length && positions[prefetchEnd].y < prefetchBottom) {
    prefetchEnd++;
  }

  const prefetchIndices: number[] = [];
  for (let i = prefetchStart; i < start; i++) prefetchIndices.push(i);
  for (let i = end; i < prefetchEnd; i++) prefetchIndices.push(i);

  return { start, end, prefetchIndices };
}
