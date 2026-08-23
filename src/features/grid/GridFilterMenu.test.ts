import { describe, expect, it } from 'vitest';
import { createStore } from 'jotai';
import { countActiveGridFilters } from './GridFilterMenu';
import { currentGridQueryAtom, gridSessionAtom } from '../../state/grid';

describe('countActiveGridFilters', () => {
  it('counts each active rule represented by the toolbar badge', () => {
    expect(countActiveGridFilters({
      rating: { value: 3, op: 'gte' },
      entity_types: ['image', 'video'],
      tags: [{ tag: 'favorite', match_mode: 'include' }],
    })).toBe(4);
  });

  it('ignores empty filter collections', () => {
    expect(countActiveGridFilters({ entity_types: [], tags: [] })).toBe(0);
  });

  it('combines toolbar filters with the existing search query', () => {
    const store = createStore();
    store.set(gridSessionAtom, {
      ...store.get(gridSessionAtom),
      searchText: 'portrait',
      filters: { rating: { value: 4, op: 'gte' }, entity_types: ['image'] },
    });

    expect(store.get(currentGridQueryAtom).filters).toEqual({
      search_text: 'portrait',
      rating: { value: 4, op: 'gte' },
      entity_types: ['image'],
    });
  });
});
