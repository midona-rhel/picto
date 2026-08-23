import { describe, expect, it } from 'vitest';
import { createStore } from 'jotai';
import { countActiveGridFilters } from './GridFilterMenu';
import { currentGridQueryAtom, gridFiltersAtom, gridSearchTextAtom } from '../../state/grid';

const emptyFilters = {
  include_tags: [],
  exclude_tags: [],
  minimum_rating: null,
  mime_prefix: null,
  text: null,
};

describe('countActiveGridFilters', () => {
  it('counts each canonical filter represented by the toolbar badge', () => {
    expect(countActiveGridFilters({
      ...emptyFilters,
      minimum_rating: 3,
      mime_prefix: 'image/',
      include_tags: ['favorite'],
    })).toBe(3);
  });

  it('ignores empty filters', () => {
    expect(countActiveGridFilters(emptyFilters)).toBe(0);
  });

  it('combines toolbar filters with the existing search query', () => {
    const store = createStore();
    store.set(gridSearchTextAtom, 'portrait');
    store.set(gridFiltersAtom, {
      ...emptyFilters,
      minimum_rating: 4,
      mime_prefix: 'image/',
    });

    expect(store.get(currentGridQueryAtom).filters).toEqual({
      ...emptyFilters,
      minimum_rating: 4,
      mime_prefix: 'image/',
      text: 'portrait',
    });
  });
});
