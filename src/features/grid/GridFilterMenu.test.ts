import { describe, expect, it } from 'vitest';
import type { ItemFilters } from '../../shared/types/generated/application/ItemFilters';
import { countActiveGridFilters } from './GridFilterMenu';

const emptyFilters: ItemFilters = {
  include_tags: [],
  exclude_tags: [],
  minimum_rating: null,
  mime_prefix: null,
  text: null,
};

describe('countActiveGridFilters', () => {
  it('counts each active replacement filter represented by the toolbar badge', () => {
    expect(countActiveGridFilters({
      ...emptyFilters,
      minimum_rating: 3,
      mime_prefix: 'image/',
      include_tags: ['favorite'],
      exclude_tags: ['spoiler'],
    })).toBe(4);
  });

  it('ignores empty filter values and independent text search', () => {
    expect(countActiveGridFilters({
      ...emptyFilters,
      text: 'portrait',
    })).toBe(0);
  });
});
