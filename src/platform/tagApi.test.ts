import { describe, expect, it, vi } from 'vitest';
import {
  deleteTag,
  deleteTagGroup,
  deleteUnusedTags,
  getNamespaceSummary,
  getUnusedTagCount,
  getTagsPaginated,
  mergeTags,
  renameTag,
  renameTagGroup,
} from './tagApi';
import { invoke } from './ipc';

vi.mock('./ipc', () => ({
  invoke: vi.fn(),
}));

describe('tagApi pagination', () => {
  it('returns the backend page and opaque cursor unchanged', async () => {
    const page = {
      tags: [{
        tag_id: 7,
        namespace_id: 2,
        namespace: 'character',
        subname: 'alice',
        active_count: 2,
        assignment_count: 2,
      }],
      next_cursor: 'backend-cursor',
      revision: 3,
    };
    vi.mocked(invoke).mockResolvedValue(page);

    await expect(getTagsPaginated({ limit: 1 })).resolves.toEqual(page);
    expect(invoke).toHaveBeenCalledWith('tags.list', {
      namespace: null,
      search: null,
      cursor: null,
      limit: 1,
    });
  });

  it('uses replacement tag commands and generated payload shapes', async () => {
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === 'tags.namespace_counts') return [
        { namespace_id: 1, name: '', tag_count: 2 },
        { namespace_id: 2, name: 'character', tag_count: 3 },
      ];
      if (command === 'tags.unused_count') return 4;
      return { revision: 4, resources: ['tags'], item_ids: [] };
    });

    await expect(getNamespaceSummary()).resolves.toEqual([
      { namespace_id: 1, name: '', tag_count: 2 },
      { namespace_id: 2, name: 'character', tag_count: 3 },
    ]);
    await expect(getUnusedTagCount()).resolves.toBe(4);
    await expect(renameTag(7, 'character:renamed')).resolves.toEqual(expect.any(Object));
    await expect(mergeTags(7, 'character:alice')).resolves.toEqual(expect.any(Object));
    await expect(deleteTag(7)).resolves.toEqual(expect.any(Object));
    await expect(deleteUnusedTags()).resolves.toEqual(expect.any(Object));
    await expect(renameTagGroup(2, 'cast')).resolves.toEqual(expect.any(Object));
    await expect(deleteTagGroup(2)).resolves.toEqual(expect.any(Object));

    expect(invoke).toHaveBeenCalledWith('tags.namespace_counts');
    expect(invoke).toHaveBeenCalledWith('tags.unused_count');
    expect(invoke).toHaveBeenCalledWith('tags.rename_or_merge', { tag_id: 7, name: 'character:renamed' });
    expect(invoke).toHaveBeenCalledWith('tags.delete', { tag_id: 7 });
    expect(invoke).toHaveBeenCalledWith('tags.delete_unused', {});
    expect(invoke).toHaveBeenCalledWith('tags.group.rename', {
      namespace_id: 2,
      name: 'cast',
    });
    expect(invoke).toHaveBeenCalledWith('tags.group.delete', { namespace_id: 2 });
  });
});
