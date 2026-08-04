import { describe, expect, it } from 'vitest';
import { nodeIdToGridScope, scopeToGridNodeId, isNonGridNodeId } from './gridScope';

describe('gridScope helpers', () => {
  it('maps grid node ids to canonical scopes', () => {
    expect(nodeIdToGridScope('system:active')).toEqual({ kind: 'system', key: 'all' });
    expect(nodeIdToGridScope('system:recent_viewed')).toEqual({ kind: 'system', key: 'recent_viewed' });
    expect(nodeIdToGridScope('folder:12')).toEqual({ kind: 'folder', id: 12 });
    expect(nodeIdToGridScope('smart:7')).toEqual({ kind: 'smart_folder', id: 7 });
    expect(nodeIdToGridScope('collection:33')).toEqual({ kind: 'collection', id: 33 });
  });

  it('keeps non-grid manager nodes out of grid scope mapping', () => {
    expect(isNonGridNodeId('system:subscriptions')).toBe(true);
    expect(isNonGridNodeId('system:tag_manager')).toBe(true);
    expect(nodeIdToGridScope('system:subscriptions')).toBeNull();
    expect(nodeIdToGridScope('system:duplicates')).toBeNull();
    expect(nodeIdToGridScope('system:tag_manager')).toBeNull();
  });

  it('maps canonical scopes back to active node ids', () => {
    expect(scopeToGridNodeId({ kind: 'system', key: 'all' })).toBe('system:active');
    expect(scopeToGridNodeId({ kind: 'system', key: 'recent_viewed' })).toBe('system:recent_viewed');
    expect(scopeToGridNodeId({ kind: 'folder', id: 5 })).toBe('folder:5');
    expect(scopeToGridNodeId({ kind: 'smart_folder', id: 8 })).toBe('smart:8');
    expect(scopeToGridNodeId({ kind: 'collection', id: 21 })).toBe('collection:21');
  });
});
