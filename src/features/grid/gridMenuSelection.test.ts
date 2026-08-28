import { describe, expect, it } from 'vitest';
import type { EntityTarget } from '../../shared/types/canonical';
import { resolveContextMenuTarget } from './gridMenuSelection';
import { compileGridQuery, createEmptyItemFilters } from '../../shared/lib/itemFilters';

describe('resolveContextMenuTarget', () => {
  it('captures the explicitly selected item IDs instead of a stale target', () => {
    const staleTarget: EntityTarget = { kind: 'explicit', root_ids: [10] };

    expect(resolveContextMenuTarget(false, staleTarget, new Set([42, 43]))).toEqual({
      kind: 'explicit',
      root_ids: [42, 43],
    });
  });

  it('preserves a query-wide selection instead of reducing it to loaded item IDs', () => {
    const queryTarget: EntityTarget = {
      kind: 'query',
      query: compileGridQuery(
        { kind: 'all' },
        createEmptyItemFilters(),
        { field: 'imported_at', direction: 'descending', random_seed: null },
      ),
      excluded_root_ids: [9],
    };

    expect(resolveContextMenuTarget(true, queryTarget, new Set([1, 2]))).toBe(queryTarget);
  });

  it('returns null for an empty explicit selection', () => {
    expect(resolveContextMenuTarget(false, null, new Set())).toBeNull();
  });
});
