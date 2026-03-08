/**
 * Arguments for grid page fetches — camelCase interface that maps to
 * the Rust `GridPageSlimQuery` (which accepts camelCase via serde aliases).
 */
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
  /** Color tolerance / max distance (1-30, lower = stricter) */
  colorAccuracy?: number | null;
  /** Free-text search query (FTS5) */
  searchText?: string | null;
  /** Seed for deterministic random ordering (Random view) */
  randomSeed?: number | null;
}

/**
 * Canonical identity for a grid query. Two queries with the same key
 * produce the same result set (modulo cursor position).
 */
export interface GridQueryKey {
  readonly folderId: number | null;
  readonly collectionEntityId: number | null;
  readonly filterFolderIds: number[] | null;
  readonly excludedFilterFolderIds: number[] | null;
  readonly folderMatchMode: 'all' | 'any' | 'exact' | null;
  readonly statusFilter: string | null;
  readonly searchTags: string[] | null;
  readonly excludedSearchTags: string[] | null;
  readonly tagMatchMode: 'all' | 'any' | 'exact' | null;
  readonly smartFolderPredicate: unknown | null;
  readonly sortField: string;
  readonly sortOrder: string;
  readonly ratingMin: number | null;
  readonly mimePrefixes: string[] | null;
  readonly colorHex: string | null;
  readonly colorAccuracy: number | null;
  readonly searchText: string | null;
  readonly randomSeed: number | null;
}

/**
 * Serializes a GridQueryKey to a stable string for identity comparison.
 */
export function serializeQueryKey(key: GridQueryKey): string {
  return JSON.stringify([
    key.folderId,
    key.collectionEntityId,
    key.filterFolderIds,
    key.excludedFilterFolderIds,
    key.folderMatchMode,
    key.statusFilter,
    key.searchTags,
    key.excludedSearchTags,
    key.tagMatchMode,
    key.smartFolderPredicate,
    key.sortField,
    key.sortOrder,
    key.ratingMin,
    key.mimePrefixes,
    key.colorHex,
    key.colorAccuracy,
    key.searchText,
    key.randomSeed,
  ]);
}

/**
 * Maps a GridQueryKey + cursor + limit to FetchGridPageArgs.
 */
export function queryKeyToFetchArgs(
  key: GridQueryKey,
  cursor: string | null,
  limit: number,
): FetchGridPageArgs {
  return {
    limit,
    cursor,
    sortField: key.sortField,
    sortOrder: key.sortOrder,
    smartFolderPredicate: key.smartFolderPredicate,
    searchTags: key.searchTags,
    searchExcludedTags: key.excludedSearchTags,
    tagMatchMode: key.tagMatchMode,
    status: key.statusFilter,
    folderIds: key.folderId
      ? [key.folderId]
      : key.filterFolderIds && key.filterFolderIds.length > 0
        ? key.filterFolderIds
        : null,
    excludedFolderIds: key.folderId ? null : key.excludedFilterFolderIds,
    folderMatchMode: key.folderId ? null : key.folderMatchMode,
    collectionEntityId: key.collectionEntityId,
    ratingMin: key.ratingMin,
    mimePrefixes: key.mimePrefixes,
    colorHex: key.colorHex,
    colorAccuracy: key.colorAccuracy,
    searchText: key.searchText,
    randomSeed: key.randomSeed,
  };
}
