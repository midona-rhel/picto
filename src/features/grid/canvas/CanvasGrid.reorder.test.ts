import { describe, expect, it } from 'vitest';
import { isOverGridItems, planFolderReorder } from './CanvasGrid';

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
