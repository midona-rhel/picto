import type { SmartFolderPredicate } from '../../features/smart-folders/components/types';
import { predicateToRust } from '../../features/smart-folders/components/types';

export interface FetchGridPageArgs {
  limit: number;
  cursor: string | null;
  sortField: string;
  sortOrder: string;
  smartFolderPredicate?: unknown | null;
  searchTags?: string[] | null;
  searchExcludedTags?: string[] | null;
  tagMatchMode?: 'all' | 'any' | 'exact' | null;
  status?: string | null;
  folderIds?: number[] | null;
  excludedFolderIds?: number[] | null;
  folderMatchMode?: 'all' | 'any' | 'exact' | null;
  collectionEntityId?: number | null;
  ratingMin?: number | null;
  mimePrefixes?: string[] | null;
  colorHex?: string | null;
  colorAccuracy?: number | null;
  searchText?: string | null;
  randomSeed?: number | null;
}

export interface GridQuery {
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

export interface GridQueryInput {
  folderId: number | null;
  collectionEntityId: number | null;
  filterFolderIds: number[] | null;
  excludedFilterFolderIds: number[] | null;
  folderMatchMode: 'all' | 'any' | 'exact' | null;
  statusFilter: string | null;
  searchTags: string[] | null;
  excludedSearchTags: string[] | null;
  tagMatchMode: 'all' | 'any' | 'exact' | null;
  smartFolderPredicate: SmartFolderPredicate | null;
  smartFolderSortField: string | null;
  smartFolderSortOrder: string | null;
  sortField: string;
  sortOrder: string;
  ratingMin: number | null;
  mimePrefixes: string[] | null;
  colorHex: string | null;
  colorAccuracy: number | null;
  searchText: string | null;
  randomSeed: number | null;
}

export function buildGridQuery(input: GridQueryInput): GridQuery {
  const effectiveSortField = input.smartFolderPredicate
    ? (input.smartFolderSortField ?? input.sortField)
    : input.sortField;
  const effectiveSortOrder = input.smartFolderPredicate
    ? (input.smartFolderSortOrder ?? input.sortOrder)
    : input.sortOrder;
  const hasSmartFolder = !!(input.smartFolderPredicate && input.smartFolderPredicate.groups.length > 0);
  const rustPredicate = hasSmartFolder ? predicateToRust(input.smartFolderPredicate!) : null;

  return {
    folderId: input.folderId ?? null,
    collectionEntityId: input.collectionEntityId ?? null,
    filterFolderIds: input.filterFolderIds ?? null,
    excludedFilterFolderIds: input.excludedFilterFolderIds ?? null,
    folderMatchMode: input.folderMatchMode ?? null,
    statusFilter: input.statusFilter ?? null,
    searchTags: input.searchTags && input.searchTags.length > 0 ? input.searchTags : null,
    excludedSearchTags: input.excludedSearchTags && input.excludedSearchTags.length > 0 ? input.excludedSearchTags : null,
    tagMatchMode: input.tagMatchMode ?? null,
    smartFolderPredicate: rustPredicate,
    sortField: effectiveSortField,
    sortOrder: effectiveSortOrder,
    ratingMin: input.ratingMin ?? null,
    mimePrefixes: input.mimePrefixes ?? null,
    colorHex: input.colorHex ?? null,
    colorAccuracy: input.colorAccuracy ?? null,
    searchText: input.searchText ?? null,
    randomSeed: input.randomSeed ?? null,
  };
}

export function serializeGridQuery(query: GridQuery): string {
  return JSON.stringify([
    query.folderId,
    query.collectionEntityId,
    query.filterFolderIds,
    query.excludedFilterFolderIds,
    query.folderMatchMode,
    query.statusFilter,
    query.searchTags,
    query.excludedSearchTags,
    query.tagMatchMode,
    query.smartFolderPredicate,
    query.sortField,
    query.sortOrder,
    query.ratingMin,
    query.mimePrefixes,
    query.colorHex,
    query.colorAccuracy,
    query.searchText,
    query.randomSeed,
  ]);
}

export function toFetchGridPageArgs(
  query: GridQuery,
  cursor: string | null,
  limit: number,
): FetchGridPageArgs {
  return {
    limit,
    cursor,
    sortField: query.sortField,
    sortOrder: query.sortOrder,
    smartFolderPredicate: query.smartFolderPredicate,
    searchTags: query.searchTags,
    searchExcludedTags: query.excludedSearchTags,
    tagMatchMode: query.tagMatchMode,
    status: query.statusFilter,
    folderIds: query.folderId
      ? [query.folderId]
      : query.filterFolderIds && query.filterFolderIds.length > 0
        ? query.filterFolderIds
        : null,
    excludedFolderIds: query.folderId ? null : query.excludedFilterFolderIds,
    folderMatchMode: query.folderId ? null : query.folderMatchMode,
    collectionEntityId: query.collectionEntityId,
    ratingMin: query.ratingMin,
    mimePrefixes: query.mimePrefixes,
    colorHex: query.colorHex,
    colorAccuracy: query.colorAccuracy,
    searchText: query.searchText,
    randomSeed: query.randomSeed,
  };
}
