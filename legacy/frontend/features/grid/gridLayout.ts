import {
  BUCKET_SIZE,
  buildBucketIndex,
  buildBucketIndexEntries,
  bucketIndexEntriesToMap,
  computeLayout,
  safeAspectRatio,
  type BucketIndexEntry,
  type LayoutItem,
  type LayoutResult,
} from './layoutMath';

export {
  BUCKET_SIZE,
  buildBucketIndex,
  buildBucketIndexEntries,
  bucketIndexEntriesToMap,
  computeLayout,
  safeAspectRatio,
  type BucketIndexEntry,
  type LayoutItem,
  type LayoutResult,
};

export const TEXT_NAME_ROW_H = 20;
export const TEXT_RESOLUTION_ROW_H = 20;
export const TEXT_AREA_H = TEXT_NAME_ROW_H + TEXT_RESOLUTION_ROW_H;

export function computeTextHeight(showName: boolean, showResolution: boolean): number {
  let height = 0;
  if (showName) height += TEXT_NAME_ROW_H;
  if (showResolution) height += TEXT_RESOLUTION_ROW_H;
  return height;
}

export function lowerBound<T>(
  items: T[],
  target: number,
  selector: (item: T) => number,
): number {
  let lo = 0;
  let hi = items.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (selector(items[mid]) < target) lo = mid + 1;
    else hi = mid;
  }
  return lo;
}
