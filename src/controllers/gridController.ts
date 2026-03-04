import { api } from '#desktop/api';
import { prefetchMetadataBatch } from '../components/image-grid/metadataPrefetch';

// Re-export types from central api types for backwards compatibility.
export type { GridPageSlimResponse } from '../types/api';
import type { GridPageSlimResponse } from '../types/api';

export interface FetchGridPageArgs {
  limit: number;
  cursor: string | null;
  sortField: string;
  sortOrder: string;
  /** Smart folder predicate for bitmap-filtered pagination */
  smartFolderPredicate?: unknown | null;
  /** Tag search strings for bitmap-filtered pagination */
  searchTags?: string[] | null;
  /** Excluded tag strings for bitmap-filtered pagination */
  searchExcludedTags?: string[] | null;
  /** Included tag matching mode */
  tagMatchMode?: 'all' | 'any' | 'exact' | null;
  /** Explicit status filter */
  status?: string | null;
  /** Folder IDs for folder-scoped grid view (single sidebar folder or multi-folder filter) */
  folderIds?: number[] | null;
  /** Excluded folder IDs for folder-scoped grid filtering */
  excludedFolderIds?: number[] | null;
  /** Included folder matching mode */
  folderMatchMode?: 'all' | 'any' | 'exact' | null;
  /** Collection entity scope — restricts to members of this collection */
  collectionEntityId?: number | null;
  /** Minimum rating filter (1-5) */
  ratingMin?: number | null;
  /** MIME prefix filters (e.g. ['image/', 'video/']) */
  mimePrefixes?: string[] | null;
  /** Dominant color hex filter */
  colorHex?: string | null;
  /** Color accuracy / max distance (5-40, lower = stricter) */
  colorAccuracy?: number | null;
  /** Free-text search query (FTS5) */
  searchText?: string | null;
  /** Seed for deterministic random ordering (Random view) */
  randomSeed?: number | null;
}

/**
 * GridController — orchestration facade for V2 grid reads and metadata warming.
 *
 * All grid scopes (All Images, smart folders, tag search) use the same
 * keyset-cursor `get_grid_page_slim` command. No OFFSET pagination.
 */
export const GridController = {
  fetchGridPage(args: FetchGridPageArgs): Promise<GridPageSlimResponse> {
    return api.grid.getPageSlim({
      limit: args.limit,
      cursor: args.cursor,
      sortField: args.sortField,
      sortOrder: args.sortOrder,
      smartFolderPredicate: args.smartFolderPredicate ?? null,
      searchTags: args.searchTags ?? null,
      searchExcludedTags: args.searchExcludedTags ?? null,
      tagMatchMode: args.tagMatchMode ?? null,
      status: args.status ?? null,
      folderIds: args.folderIds ?? null,
      excludedFolderIds: args.excludedFolderIds ?? null,
      folderMatchMode: args.folderMatchMode ?? null,
      collectionEntityId: args.collectionEntityId ?? null,
      ratingMin: args.ratingMin ?? null,
      mimePrefixes: args.mimePrefixes ?? null,
      colorHex: args.colorHex ?? null,
      colorAccuracy: args.colorAccuracy ?? null,
      searchText: args.searchText ?? null,
      randomSeed: args.randomSeed ?? null,
    });
  },

  /** Backwards-compat alias — calls fetchGridPage with no scope filters. */
  fetchDefaultGridPage(args: Omit<FetchGridPageArgs, 'smartFolderPredicate' | 'searchTags' | 'status'>): Promise<GridPageSlimResponse> {
    return this.fetchGridPage(args);
  },

  prefetchVisibleMetadata(hashes: string[]): Promise<void> {
    return prefetchMetadataBatch(hashes);
  },
};
