import type { MasonryImageItem } from './shared';

function compareNullableNumber(
  a: number | null | undefined,
  b: number | null | undefined,
): number {
  const av = typeof a === 'number' ? a : Number.NEGATIVE_INFINITY;
  const bv = typeof b === 'number' ? b : Number.NEGATIVE_INFINITY;
  return av - bv;
}

function compareByField(a: MasonryImageItem, b: MasonryImageItem, sortField: string): number {
  switch (sortField) {
    case 'size':
      return a.size - b.size;
    case 'rating':
      return compareNullableNumber(a.rating, b.rating);
    case 'name':
      return (a.name ?? '').localeCompare(b.name ?? '');
    case 'mime':
      return a.mime.localeCompare(b.mime);
    case 'date_created':
    case 'date_modified':
    case 'date_added':
    default:
      // Grid slim items only carry date_added; date_created/date_modified
      // fall back to date_added for live-sort approximation.
      return a.date_added.localeCompare(b.date_added);
  }
}

export function sortLiveImages(
  images: MasonryImageItem[],
  sortField: string,
  sortOrder: 'asc' | 'desc',
): MasonryImageItem[] {
  const sorted = [...images];
  sorted.sort((a, b) => {
    const base = compareByField(a, b, sortField);
    if (base !== 0) return sortOrder === 'desc' ? -base : base;
    // Deterministic tie-breakers to avoid visual jitter.
    const dateAddedCmp = a.date_added.localeCompare(b.date_added);
    if (dateAddedCmp !== 0) return sortOrder === 'desc' ? -dateAddedCmp : dateAddedCmp;
    return a.hash.localeCompare(b.hash);
  });
  return sorted;
}
