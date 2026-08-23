import { describe, expect, it } from 'vitest';
import { hasSameEntityOrder } from './gridItemIdentity';

describe('hasSameEntityOrder', () => {
  it('preserves identity across metadata-only object replacement', () => {
    const replacedItems = [
      { entity_hash: 'a', name: 'updated' },
      { entity_hash: 'b', name: 'updated again' },
    ];
    expect(hasSameEntityOrder(['a', 'b'], replacedItems)).toBe(true);
  });

  it('detects insertion, deletion, and reorder', () => {
    expect(hasSameEntityOrder(['a', 'b'], [{ entity_hash: 'a' }])).toBe(false);
    expect(hasSameEntityOrder(['a'], [{ entity_hash: 'new' }, { entity_hash: 'a' }])).toBe(false);
    expect(hasSameEntityOrder(['a', 'b'], [{ entity_hash: 'b' }, { entity_hash: 'a' }])).toBe(false);
  });
});
