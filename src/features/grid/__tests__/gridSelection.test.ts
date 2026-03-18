import { describe, expect, it } from 'vitest';

import { buildVirtualSelectAllBaseSpec } from '../hooks/useGridSelection';

describe('buildVirtualSelectAllBaseSpec', () => {
  it('prioritizes explicit folder scope over filter folders', () => {
    const spec = buildVirtualSelectAllBaseSpec({
      searchTags: ['fox'],
      statusFilter: 'active',
      folderId: 42,
      filterFolderIds: [1, 2],
      excludedFilterFolderIds: [7],
      folderMatchMode: 'any',
    });

    expect(spec.mode).toBe('all_results');
    expect(spec.scope).toEqual({ kind: 'folder', folder_id: 42 });
    expect(spec.filters.search_tags).toEqual(['fox']);
    expect(spec.filters.folder_ids).toBeNull();
    expect(spec.filters.excluded_folder_ids).toBeNull();
    expect(spec.filters.folder_match_mode).toBeNull();
  });

  it('uses include/exclude folder filters when no explicit folder scope is active', () => {
    const spec = buildVirtualSelectAllBaseSpec({
      filterFolderIds: [3, 4],
      excludedFilterFolderIds: [9],
      folderMatchMode: 'exact',
    });

    expect(spec.scope).toEqual({ kind: 'system', system_key: 'all' });
    expect(spec.filters.folder_ids).toEqual([3, 4]);
    expect(spec.filters.excluded_folder_ids).toEqual([9]);
    expect(spec.filters.folder_match_mode).toBe('exact');
  });

  it('preserves collection scope in the selection query spec', () => {
    const spec = buildVirtualSelectAllBaseSpec({
      collectionEntityId: 77,
      statusFilter: 'active',
    });

    expect(spec.scope).toEqual({ kind: 'collection', collection_entity_id: 77 });
    expect(spec.filters.folder_ids).toBeNull();
  });
});
