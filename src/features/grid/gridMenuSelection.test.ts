import { describe, expect, it } from 'vitest';
import type { ItemTarget } from '../../shared/types/generated/application/ItemTarget';
import { resolveContextMenuTarget } from './gridMenuSelection';

describe('resolveContextMenuTarget', () => {
  it('captures the explicitly selected item IDs instead of a stale target', () => {
    const staleTarget: ItemTarget = { kind: 'explicit', item_ids: [10] };

    expect(resolveContextMenuTarget(false, staleTarget, new Set([42, 43]))).toEqual({
      kind: 'explicit',
      item_ids: [42, 43],
    });
  });

  it('preserves a query-wide selection instead of reducing it to loaded item IDs', () => {
    const queryTarget: ItemTarget = {
      kind: 'query',
      query: {
        scope: { kind: 'all' },
        filters: {
          include_tags: [],
          exclude_tags: [],
          minimum_rating: null,
          mime_prefix: null,
          text: null,
        },
        sort: { field: 'imported_at', direction: 'descending', random_seed: null },
      },
      excluded_item_ids: [9],
    };

    expect(resolveContextMenuTarget(true, queryTarget, new Set([1, 2]))).toBe(queryTarget);
  });

  it('returns null for an empty explicit selection', () => {
    expect(resolveContextMenuTarget(false, null, new Set())).toBeNull();
  });
});
