import { describe, expect, it } from 'vitest';
import type { EntityTarget } from '../../shared/types/canonical';
import { resolveContextMenuTarget } from './gridMenuSelection';

describe('resolveContextMenuTarget', () => {
  it('captures the explicitly selected hashes instead of a stale target', () => {
    const staleTarget: EntityTarget = { kind: 'entity_hashes', entity_hashes: ['old'] };

    expect(resolveContextMenuTarget(false, staleTarget, new Set(['right-clicked']))).toEqual({
      kind: 'entity_hashes',
      entity_hashes: ['right-clicked'],
    });
  });

  it('preserves a query-wide selection instead of reducing it to loaded hashes', () => {
    const queryTarget: EntityTarget = {
      kind: 'query_results',
      query: { base_scope: { kind: 'system', key: 'all' } },
      excluded_entity_hashes: ['excluded'],
    };

    expect(resolveContextMenuTarget(true, queryTarget, new Set(['loaded']))).toBe(queryTarget);
  });

  it('returns null for an empty explicit selection', () => {
    expect(resolveContextMenuTarget(false, null, new Set())).toBeNull();
  });
});
