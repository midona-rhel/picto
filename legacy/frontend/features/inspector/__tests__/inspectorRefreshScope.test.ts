import { describe, expect, it } from 'vitest';
import { inspectorNeedsRefresh } from '../inspectorRefreshScope';
import type { ResourceKey } from '../../../shared/types/backendState';

function keys(...values: ResourceKey[]): Set<ResourceKey> {
  return new Set(values);
}

describe('inspectorNeedsRefresh', () => {
  it('refreshes virtual selection on selection/current', () => {
    expect(inspectorNeedsRefresh({
      selectedHashes: [],
      hasVirtualSelection: true,
      hasSelectedCollection: false,
    }, keys('selection/current'))).toBe(true);
  });

  it('refreshes single-file inspector on matching metadata hash', () => {
    expect(inspectorNeedsRefresh({
      selectedHashes: ['a'],
      hasVirtualSelection: false,
      hasSelectedCollection: false,
    }, keys('metadata/hash:a'))).toBe(true);
  });

  it('does not refresh single-file inspector for unrelated metadata hash', () => {
    expect(inspectorNeedsRefresh({
      selectedHashes: ['a'],
      hasVirtualSelection: false,
      hasSelectedCollection: false,
    }, keys('metadata/hash:b'))).toBe(false);
  });

  it('refreshes multi-file inspector on selection/current', () => {
    expect(inspectorNeedsRefresh({
      selectedHashes: ['a', 'b'],
      hasVirtualSelection: false,
      hasSelectedCollection: false,
    }, keys('selection/current'))).toBe(true);
  });

  it('does not refresh selected collections from file/media refresh targets', () => {
    expect(inspectorNeedsRefresh({
      selectedHashes: ['a'],
      hasVirtualSelection: false,
      hasSelectedCollection: true,
    }, keys('selection/current', 'metadata/hash:a'))).toBe(false);
  });
});
