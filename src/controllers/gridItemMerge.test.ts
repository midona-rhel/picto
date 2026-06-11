import { describe, expect, it } from 'vitest';
import { gridItemComparator, sortedMergeGridItems } from './gridItemMerge';
import type { CanonicalEntityGridItem } from '../shared/types/canonical';
import type { SortField, SortDirection } from '../state/grid';

let nextId = 1;

function makeItem(overrides: Partial<CanonicalEntityGridItem> = {}): CanonicalEntityGridItem {
  const id = nextId++;
  return {
    entity_id: id,
    entity_hash: `hash-${id}`,
    thumbnail_hash: `hash-${id}`,
    entity_kind: 'single',
    name: `item-${id}`,
    mime_type: 'image/png',
    pixel_width: 100,
    pixel_height: 100,
    status: 1,
    rating: null,
    date_added: '2026-01-01T00:00:00Z',
    date_created: '2026-01-01T00:00:00Z',
    date_modified: '2026-01-01T00:00:00Z',
    has_thumbnail: true,
    member_count: null,
    duration_ms: null,
    frame_count: null,
    has_audio: false,
    dominant_color_hex: null,
    size_bytes: 1000,
    ...overrides,
  };
}

/** Reference implementation — the previous binary-search + splice algorithm. */
function referenceSortedMerge(
  existing: CanonicalEntityGridItem[],
  newItems: CanonicalEntityGridItem[],
  field: SortField,
  dir: SortDirection,
): CanonicalEntityGridItem[] {
  if (field === 'size_bytes') return [...existing, ...newItems];
  const cmp = gridItemComparator(field, dir);
  const merged = [...existing];
  for (const item of newItems) {
    let lo = 0;
    let hi = merged.length;
    while (lo < hi) {
      const mid = (lo + hi) >>> 1;
      if (cmp(merged[mid], item) <= 0) lo = mid + 1;
      else hi = mid;
    }
    merged.splice(lo, 0, item);
  }
  return merged;
}

/** Deterministic PRNG so failures reproduce. */
function mulberry32(seed: number): () => number {
  let a = seed;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

describe('sortedMergeGridItems', () => {
  it('merges into an empty array', () => {
    const items = [makeItem({ date_added: '2026-01-02' }), makeItem({ date_added: '2026-01-01' })];
    const result = sortedMergeGridItems([], items, 'date_added', 'asc');
    expect(result.map((i) => i.date_added)).toEqual(['2026-01-01', '2026-01-02']);
  });

  it('returns existing unchanged when newItems is empty', () => {
    const existing = [makeItem(), makeItem()];
    expect(sortedMergeGridItems(existing, [], 'date_added', 'asc')).toEqual(existing);
  });

  it('handles both arrays empty', () => {
    expect(sortedMergeGridItems([], [], 'name', 'desc')).toEqual([]);
  });

  it('places unsorted new items at their correct positions', () => {
    const existing = [
      makeItem({ date_added: '2026-01-01' }),
      makeItem({ date_added: '2026-01-03' }),
      makeItem({ date_added: '2026-01-05' }),
    ];
    const incoming = [
      makeItem({ date_added: '2026-01-04' }),
      makeItem({ date_added: '2026-01-02' }),
      makeItem({ date_added: '2026-01-06' }),
    ];
    const result = sortedMergeGridItems(existing, incoming, 'date_added', 'asc');
    expect(result.map((i) => i.date_added)).toEqual([
      '2026-01-01', '2026-01-02', '2026-01-03', '2026-01-04', '2026-01-05', '2026-01-06',
    ]);
  });

  it('treats null fields as the largest value (last in asc, first in desc)', () => {
    const existing = [makeItem({ rating: 1 }), makeItem({ rating: 3 })];
    const incoming = [makeItem({ rating: null }), makeItem({ rating: 2 })];
    const asc = sortedMergeGridItems(existing, incoming, 'rating', 'asc');
    expect(asc.map((i) => i.rating)).toEqual([1, 2, 3, null]);
    const descExisting = [makeItem({ rating: 3 }), makeItem({ rating: 1 })];
    const desc = sortedMergeGridItems(descExisting, incoming, 'rating', 'desc');
    expect(desc.map((i) => i.rating)).toEqual([null, 3, 2, 1]);
  });

  it('keeps existing items before equal new items, and new items in input order', () => {
    const e1 = makeItem({ date_added: '2026-01-01', name: 'e1' });
    const e2 = makeItem({ date_added: '2026-01-01', name: 'e2' });
    const n1 = makeItem({ date_added: '2026-01-01', name: 'n1' });
    const n2 = makeItem({ date_added: '2026-01-01', name: 'n2' });
    const result = sortedMergeGridItems([e1, e2], [n1, n2], 'date_added', 'asc');
    expect(result.map((i) => i.name)).toEqual(['e1', 'e2', 'n1', 'n2']);
  });

  it('appends for size_bytes sort', () => {
    const existing = [makeItem({ size_bytes: 5 })];
    const incoming = [makeItem({ size_bytes: 1 }), makeItem({ size_bytes: 9 })];
    const result = sortedMergeGridItems(existing, incoming, 'size_bytes', 'asc');
    expect(result).toEqual([...existing, ...incoming]);
  });

  it('matches the reference splice-based algorithm on randomized inputs', () => {
    const rand = mulberry32(42);
    const fields: SortField[] = ['date_added', 'date_created', 'date_modified', 'name', 'rating', 'duration'];
    const dirs: SortDirection[] = ['asc', 'desc'];
    for (const field of fields) {
      for (const dir of dirs) {
        const cmp = gridItemComparator(field, dir);
        const existing = Array.from({ length: 50 }, () => makeItem({
          date_added: `2026-01-${String(1 + Math.floor(rand() * 28)).padStart(2, '0')}`,
          date_created: `2026-02-${String(1 + Math.floor(rand() * 28)).padStart(2, '0')}`,
          date_modified: `2026-03-${String(1 + Math.floor(rand() * 28)).padStart(2, '0')}`,
          name: rand() < 0.1 ? null : `n${Math.floor(rand() * 20)}`,
          rating: rand() < 0.2 ? null : Math.floor(rand() * 5),
          duration_ms: rand() < 0.3 ? null : Math.floor(rand() * 10000),
        })).sort(cmp);
        const incoming = Array.from({ length: 30 }, () => makeItem({
          date_added: `2026-01-${String(1 + Math.floor(rand() * 28)).padStart(2, '0')}`,
          date_created: `2026-02-${String(1 + Math.floor(rand() * 28)).padStart(2, '0')}`,
          date_modified: `2026-03-${String(1 + Math.floor(rand() * 28)).padStart(2, '0')}`,
          name: rand() < 0.1 ? null : `n${Math.floor(rand() * 20)}`,
          rating: rand() < 0.2 ? null : Math.floor(rand() * 5),
          duration_ms: rand() < 0.3 ? null : Math.floor(rand() * 10000),
        }));
        const expected = referenceSortedMerge(existing, incoming, field, dir);
        const actual = sortedMergeGridItems(existing, incoming, field, dir);
        // Element-wise identical: both algorithms keep existing items first
        // on ties and preserve input order among equal new items.
        expect(actual.map((i) => i.entity_id)).toEqual(expected.map((i) => i.entity_id));
        for (let idx = 1; idx < actual.length; idx++) {
          expect(cmp(actual[idx - 1], actual[idx])).toBeLessThanOrEqual(0);
        }
      }
    }
  });
});
