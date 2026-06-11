/**
 * Sorted-merge helpers for grid items — pure functions, no store access.
 */

import type { CanonicalEntityGridItem } from '../shared/types/canonical';
import type { SortField, SortDirection } from '../state/grid';

/** Build a comparator for grid items based on the current sort field/direction. */
export function gridItemComparator(field: SortField, dir: SortDirection): (a: CanonicalEntityGridItem, b: CanonicalEntityGridItem) => number {
  const sign = dir === 'asc' ? 1 : -1;
  return (a, b) => {
    let av: string | number | null;
    let bv: string | number | null;
    switch (field) {
      case 'date_added': av = a.date_added; bv = b.date_added; break;
      case 'date_created': av = a.date_created; bv = b.date_created; break;
      case 'date_modified': av = a.date_modified; bv = b.date_modified; break;
      case 'name': av = a.name; bv = b.name; break;
      case 'rating': av = a.rating; bv = b.rating; break;
      case 'duration': av = a.duration_ms; bv = b.duration_ms; break;
      default: return 0; // size_bytes not on grid item — can't sort
    }
    if (av == null && bv == null) return 0;
    if (av == null) return sign;
    if (bv == null) return -sign;
    if (av < bv) return -sign;
    if (av > bv) return sign;
    return 0;
  };
}

/**
 * Merge new items into an already-sorted array at their correct positions.
 * New items may arrive unsorted; they are sorted first, then merged in a
 * single pass — O((n+m) + m log m). On ties, existing items come first and
 * new items keep their relative input order.
 */
export function sortedMergeGridItems(
  existing: CanonicalEntityGridItem[],
  newItems: CanonicalEntityGridItem[],
  field: SortField,
  dir: SortDirection,
): CanonicalEntityGridItem[] {
  if (field === 'size_bytes') {
    // size_bytes not available on grid item — append at end
    return [...existing, ...newItems];
  }
  const cmp = gridItemComparator(field, dir);
  const sortedNew = [...newItems].sort(cmp);
  const merged: CanonicalEntityGridItem[] = new Array(existing.length + sortedNew.length);
  let i = 0;
  let j = 0;
  let k = 0;
  while (i < existing.length && j < sortedNew.length) {
    merged[k++] = cmp(existing[i], sortedNew[j]) <= 0 ? existing[i++] : sortedNew[j++];
  }
  while (i < existing.length) merged[k++] = existing[i++];
  while (j < sortedNew.length) merged[k++] = sortedNew[j++];
  return merged;
}
