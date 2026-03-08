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
    expect(spec.search_tags).toEqual(['fox']);
    expect(spec.folder_ids).toEqual([42]);
    expect(spec.excluded_folder_ids).toBeNull();
    expect(spec.folder_match_mode).toBeNull();
  });

  it('uses include/exclude folder filters when no explicit folder scope is active', () => {
    const spec = buildVirtualSelectAllBaseSpec({
      filterFolderIds: [3, 4],
      excludedFilterFolderIds: [9],
      folderMatchMode: 'exact',
    });

    expect(spec.folder_ids).toEqual([3, 4]);
    expect(spec.excluded_folder_ids).toEqual([9]);
    expect(spec.folder_match_mode).toBe('exact');
  });
});
