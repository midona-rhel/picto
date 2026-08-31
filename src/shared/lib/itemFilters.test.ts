import { describe, expect, it } from 'vitest';
import { compileGridQuery, createEmptyItemFilters, DEFAULT_COLOR_DELTA_E } from './itemFilters';
import type { FilterExpr } from '../types/canonical';

const sort = { field: 'imported_at', direction: 'descending', random_seed: null } as const;

function clauses(query: ReturnType<typeof compileGridQuery>): FilterExpr[] {
  return query.view.filter.kind === 'all' ? query.view.filter.value : [query.view.filter];
}

function textQueries(query: ReturnType<typeof compileGridQuery>): string[] {
  return clauses(query).flatMap((expression) => (
    expression.kind === 'clause' && expression.value.clause === 'text'
      ? [expression.value.query]
      : []
  ));
}

describe('compileGridQuery text boundary', () => {
  it('omits global text until three trimmed Unicode characters are present', () => {
    for (const value of ['', '   ', 'a', 'ab', ' 猫 ']) {
      expect(textQueries(compileGridQuery(
        { kind: 'all' },
        createEmptyItemFilters(),
        sort,
        value,
      ))).toEqual([]);
    }

    expect(textQueries(compileGridQuery(
      { kind: 'all' },
      createEmptyItemFilters(),
      sort,
      ' 猫犬鳥 ',
    ))).toEqual(['猫犬鳥']);
  });

  it('keeps structured filters while omitting short text clauses', () => {
    const filters = {
      ...createEmptyItemFilters(),
      include_tags: [{ tag_id: 17, name: 'creator:example' }],
      include_folder_ids: [23],
      ratings: [5],
    };
    const query = compileGridQuery({ kind: 'inbox' }, filters, sort, 'ab');

    expect(query.scope).toEqual({ kind: 'inbox' });
    expect(textQueries(query)).toEqual([]);
    expect(clauses(query)).toContainEqual({
      kind: 'clause',
      value: { clause: 'tags', tag_ids: [17], mode: 'any' },
    });
    expect(clauses(query)).toContainEqual({
      kind: 'clause',
      value: { clause: 'folders', folder_ids: [23], mode: 'any' },
    });
    expect(clauses(query)).toContainEqual({
      kind: 'clause',
      value: { clause: 'ratings', ratings: ['five'] },
    });
  });
});

describe('compileGridQuery MIME facets', () => {
  it('combines broad families and concrete formats in one additive clause', () => {
    const query = compileGridQuery(
      { kind: 'all' },
      {
        ...createEmptyItemFilters(),
        include_mime_types: ['image/png', 'video/*'],
        exclude_mime_types: ['image/gif'],
      },
      sort,
    );

    expect(clauses(query)).toContainEqual({
      kind: 'clause',
      value: { clause: 'mime', values: ['image/png'], families: ['video'] },
    });
    expect(clauses(query)).toContainEqual({
      kind: 'not',
      value: {
        kind: 'clause',
        value: { clause: 'mime', values: ['image/gif'], families: [] },
      },
    });
  });
});

describe('compileGridQuery color tolerance', () => {
  it('uses the broader default and preserves an explicit bounded tolerance', () => {
    const filters = { ...createEmptyItemFilters(), color_hex: '#336699' };
    const defaultQuery = compileGridQuery({ kind: 'all' }, filters, sort);
    const defaultColor = clauses(defaultQuery).find((expression) => (
      expression.kind === 'clause' && expression.value.clause === 'color'
    ));
    expect(defaultColor?.kind === 'clause' && defaultColor.value.clause === 'color'
      ? defaultColor.value.delta_e : null).toBe(DEFAULT_COLOR_DELTA_E);

    const explicitQuery = compileGridQuery(
      { kind: 'all' },
      { ...filters, color_delta_e: 24 },
      sort,
    );
    const explicitColor = clauses(explicitQuery).find((expression) => (
      expression.kind === 'clause' && expression.value.clause === 'color'
    ));
    expect(explicitColor?.kind === 'clause' && explicitColor.value.clause === 'color'
      ? explicitColor.value.delta_e : null).toBe(24);
  });
});
