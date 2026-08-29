import { describe, expect, it, vi } from 'vitest';

const getTagsPaginated = vi.hoisted(() => vi.fn());
const getTagsById = vi.hoisted(() => vi.fn());
vi.mock('../../../platform/tagApi', () => ({ getTagsPaginated, getTagsById }));

import { compileSmartFolderPredicate, editorPredicateFromFilter } from './queryModel';

describe('canonical smart-folder query editor', () => {
  it('compiles file-size units to exact byte ranges', async () => {
    await expect(compileSmartFolderPredicate({
      groups: [{
        match_mode: 'all',
        rules: [{ field: 'file_size', op: 'eq', value: 2, unit: 'GB' }],
      }],
    })).resolves.toEqual({
      kind: 'all',
      value: [{
        kind: 'all',
        value: [{
          kind: 'clause',
          value: {
            clause: 'total_size',
            minimum_bytes: 2 * 1024 * 1024 * 1024,
            maximum_bytes: 2 * 1024 * 1024 * 1024,
          },
        }],
      }],
    });
  });

  it('resolves namespaced display tokens to stable tag IDs', async () => {
    getTagsPaginated.mockResolvedValue({
      tags: [{ tag_id: 31, namespace: 'creator', subname: 'huffslove' }],
      next_cursor: null,
      revision: 1,
    });

    await expect(compileSmartFolderPredicate({
      groups: [{
        match_mode: 'all',
        rules: [{ field: 'tags', op: 'include_any', values: ['creator:huffslove'] }],
      }],
    })).resolves.toEqual({
      kind: 'all',
      value: [{
        kind: 'all',
        value: [{ kind: 'clause', value: { clause: 'tags', tag_ids: [31], mode: 'any' } }],
      }],
    });
    expect(getTagsPaginated).toHaveBeenCalledWith({
      namespace: 'creator',
      search: 'huffslove',
      limit: 100,
    });
  });

  it('decodes stable tag IDs for rule editing', async () => {
    getTagsById.mockResolvedValue([
      { tag_id: 31, namespace: 'creator', subname: 'huffslove' },
    ]);

    await expect(editorPredicateFromFilter({
      kind: 'all',
      value: [{
        kind: 'all',
        value: [{ kind: 'clause', value: { clause: 'tags', tag_ids: [31], mode: 'any' } }],
      }],
    })).resolves.toEqual({
      groups: [{
        match_mode: 'all',
        negate: false,
        rules: [{ field: 'tags', op: 'include_any', values: ['creator:huffslove'] }],
      }],
    });
  });
});
