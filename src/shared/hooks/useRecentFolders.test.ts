import { beforeEach, describe, expect, it } from 'vitest';
import {
  readRecentFolderIds,
  recordRecentFolderUse,
  setRecentFoldersLibrary,
} from './useRecentFolders';

describe('recent folders', () => {
  beforeEach(() => localStorage.clear());

  it('keeps independent MRU lists for each library', () => {
    setRecentFoldersLibrary('/libraries/one.library');
    recordRecentFolderUse([7, 8]);
    recordRecentFolderUse([7]);
    expect(readRecentFolderIds()).toEqual([7, 8]);

    setRecentFoldersLibrary('/libraries/two.library');
    expect(readRecentFolderIds()).toEqual([]);
    recordRecentFolderUse([3]);

    setRecentFoldersLibrary('/libraries/one.library');
    expect(readRecentFolderIds()).toEqual([7, 8]);
  });

  it('ignores invalid IDs and caps the stored list', () => {
    setRecentFoldersLibrary('/libraries/one.library');
    recordRecentFolderUse([1, 2, 0, Number.NaN, 3], 2);
    expect(readRecentFolderIds()).toEqual([1, 2]);
  });
});
