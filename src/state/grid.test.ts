import { describe, expect, it } from 'vitest';
import { gridSessionAtom, reduceGridSession } from './grid';
import { createStore } from 'jotai';

describe('reduceGridSession', () => {
  const initial = () => createStore().get(gridSessionAtom);

  it('owns query changes without replacing loaded pages', () => {
    const before = initial();
    const searched = reduceGridSession(before, { type: 'search', text: 'portrait' });
    const filtered = reduceGridSession(searched, {
      type: 'filter',
      filters: { rating: { value: 4, op: 'gte' } },
    });
    const sorted = reduceGridSession(filtered, { type: 'sort', field: 'duration', direction: 'asc' });

    expect(sorted.query.filters).toEqual({
      search_text: 'portrait',
      rating: { value: 4, op: 'gte' },
    });
    expect(sorted.query.sort).toEqual({ field: 'duration', direction: 'asc' });
    expect(sorted.pages).toBe(before.pages);
  });

  it('changes scope and view through the same typed intent reducer', () => {
    const navigated = reduceGridSession(initial(), { type: 'navigate', scope: { kind: 'folder', id: 9 } });
    const viewed = reduceGridSession(navigated, { type: 'view', patch: { showSubfolders: false } });

    expect(viewed.query.base_scope).toEqual({ kind: 'folder', id: 9 });
    expect(viewed.query.filters).toBeUndefined();
    expect(viewed.view.showSubfolders).toBe(false);
  });
});
