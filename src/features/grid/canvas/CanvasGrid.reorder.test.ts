import { describe, expect, it } from 'vitest';
import { isOverGridItems, nativeDragHashes, planFolderReorder } from './CanvasGrid';

it('exports every selected physical file, not only the three drag-preview thumbnails', () => {
  const items = Array.from({ length: 6 }, (_, i) => ({ root_id: i + 1, content_hash: `hash-${i}` }));
  expect(nativeDragHashes([1, 2, 3, 4, 5, 6], items)).toEqual(items.map(item => item.content_hash));
  expect(nativeDragHashes([1, 7, 8], [...items, { root_id: 7, content_hash: 'hash-0' }])).toEqual(['hash-0']);
});

describe('planFolderReorder', () => {
  it('moves a selected group block without changing its internal order', () => {
    expect(planFolderReorder([1, 2, 3, 4, 5], new Set([2, 3]), 4, 'right'))
      .toEqual([1, 4, 5, 2, 3]);
  });

  it('does not persist a no-op drop inside the selected block', () => {
    expect(planFolderReorder([1, 2, 3, 4], new Set([2, 3]), 2, 'right'))
      .toEqual([]);
  });
});

describe('isOverGridItems', () => {
  it('suppresses reorder feedback over the subfolder header', () => {
    expect(isOverGridItems(-1)).toBe(false);
    expect(isOverGridItems(0)).toBe(true);
  });
});
