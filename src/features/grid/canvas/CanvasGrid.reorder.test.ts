import { describe, expect, it, vi } from 'vitest';
import type { EntityTarget } from '../../../shared/types/canonical';
import { compileGridQuery, createEmptyItemFilters } from '../../../shared/lib/itemFilters';
import { isOverGridItems, localNativeDragHashes, planFolderReorder, resolveNativeDragHashes } from './CanvasGrid';

it('resolves native drag files from the canonical selection target, including unloaded roots', async () => {
  const target: EntityTarget = {
    kind: 'range',
    query: compileGridQuery(
      { kind: 'all' },
      createEmptyItemFilters(),
      { field: 'imported_at', direction: 'descending', random_seed: null },
    ),
    spans: [{ anchor_root_id: 1, focus_root_id: 999, anchor_index: 0, focus_index: 998 }],
    included_root_ids: [],
    excluded_root_ids: [],
  };
  const resolve = vi.fn().mockResolvedValue([
    { file_hash: 'loaded-hash', path: '/library/loaded.jpg' },
    { file_hash: 'unloaded-hash', path: '/library/unloaded.jpg' },
    { file_hash: 'loaded-hash', path: '/library/duplicate.jpg' },
  ]);

  await expect(resolveNativeDragHashes(target, resolve)).resolves
    .toEqual(['loaded-hash', 'unloaded-hash']);
  expect(resolve).toHaveBeenCalledWith(target);
});

it('maps reusable grid media IDs to their physical hashes without treating them as roots', () => {
  const members = [
    { root_id: 1001, content_hash: 'member-a' },
    { root_id: 1002, content_hash: 'member-b' },
  ];

  expect(localNativeDragHashes([1002, 1001], members)).toEqual(['member-b', 'member-a']);
});

it('does not fall back to loaded files when native drag resolution fails', async () => {
  const failure = new Error('library changed');
  const resolve = vi.fn().mockRejectedValue(failure);

  await expect(resolveNativeDragHashes(
    { kind: 'explicit', root_ids: [1] },
    resolve,
  )).rejects.toBe(failure);
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
