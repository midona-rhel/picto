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
    case 'view_count':
      return a.view_count - b.view_count;
    case 'name':
      return (a.name ?? '').localeCompare(b.name ?? '');
    case 'mime':
      return a.mime.localeCompare(b.mime);
    case 'imported_at':
    default:
      return a.imported_at.localeCompare(b.imported_at);
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
    const importedAtCmp = a.imported_at.localeCompare(b.imported_at);
    if (importedAtCmp !== 0) return sortOrder === 'desc' ? -importedAtCmp : importedAtCmp;
    return a.hash.localeCompare(b.hash);
  });
  return sorted;
}
