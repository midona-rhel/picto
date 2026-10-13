import { describe, expect, it, vi } from 'vitest';
import { matchRank, rankMatches } from './searchRank';

describe('matchRank', () => {
  it('orders exact, prefix, word start, then substring', () => {
    expect(matchRank('twokinds', 'twokinds')).toBe(0);
    expect(matchRank('twokinds_comic', 'twokinds')).toBe(1);
    expect(matchRank('amelia_(twokinds)', 'twokinds')).toBe(2);
    expect(matchRank('notwokinds', 'twokinds')).toBe(3);
    expect(matchRank('keidran', 'twokinds')).toBeNull();
  });

  it('finds a word start after an earlier mid-word match', () => {
    expect(matchRank('catcat cat', 'cat')).toBe(1);
    expect(matchRank('bobcat cat', 'cat')).toBe(2);
  });
});

describe('rankMatches', () => {
  const names = ['amelia_(twokinds)', 'brutus_(twokinds)', 'notwokinds', 'twokinds', 'twokinds_comic', 'keidran'];

  it('puts the exact match first and keeps input order within a rank', () => {
    expect(rankMatches(names, ' TwoKinds ', (name) => [name])).toEqual([
      'twokinds',
      'twokinds_comic',
      'amelia_(twokinds)',
      'brutus_(twokinds)',
      'notwokinds',
    ]);
  });

  it('uses the best rank over several keys', () => {
    const items = [{ label: 'Archive box', name: 'box' }, { label: 'Box', name: 'square' }];
    expect(rankMatches(items, 'box', (item) => [item.label, item.name]).map((item) => item.label))
      .toEqual(['Archive box', 'Box']);
  });

  it('returns everything for a blank query', () => {
    expect(rankMatches(names, '  ', (name) => [name])).toEqual(names);
  });

  it.each(['tr', 'az'])('keeps ASCII I matching independent of the %s runtime locale', (locale) => {
    const lower = String.prototype.toLocaleLowerCase;
    const localized = vi.spyOn(String.prototype, 'toLocaleLowerCase').mockImplementation(function (this: string) {
      return lower.call(this, locale);
    });
    try {
      const items = ['AI tags', 'Images', 'icon', '\u0131rmak'];
      for (const query of ['i', ' I ']) {
        expect(rankMatches(items, query, (name) => [name])).toEqual(['Images', 'icon', 'AI tags']);
      }
    } finally {
      localized.mockRestore();
    }
  });
});
