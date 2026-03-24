import type { SmartFolderPredicate } from '../../features/smart-folders/components/types';
import { predicateToRust } from '../../features/smart-folders/components/types';
import type {
  GridFilterSpec,
  GridPageSlimQuery,
  GridScopeSpec,
  GridSortSpec,
  GridSystemScopeKey,
} from '../../shared/types/api/core';

export type FetchGridPageArgs = GridPageSlimQuery;

export type GridQuery = Readonly<Pick<GridPageSlimQuery, 'scope' | 'filters' | 'sort'>>;

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
  collectionsOnly: boolean | null;
  colorHex: string | null;
  colorAccuracy: number | null;
  searchText: string | null;
  randomSeed: number | null;
  similarHashes: string[] | null;
}

type ScopeQueryInput = Partial<Pick<
  GridQueryInput,
  | 'smartFolderPredicate'
  | 'statusFilter'
  | 'collectionEntityId'
  | 'folderId'
  | 'similarHashes'
>>;

type FilterQueryInput = Partial<Pick<
  GridQueryInput,
  | 'searchTags'
  | 'excludedSearchTags'
  | 'tagMatchMode'
  | 'folderId'
  | 'filterFolderIds'
  | 'excludedFilterFolderIds'
  | 'folderMatchMode'
  | 'ratingMin'
  | 'mimePrefixes'
  | 'collectionsOnly'
  | 'colorHex'
  | 'colorAccuracy'
  | 'searchText'
>>;

type SortQueryInput = Partial<Pick<
  GridQueryInput,
  | 'smartFolderPredicate'
  | 'smartFolderSortField'
  | 'smartFolderSortOrder'
  | 'sortField'
  | 'sortOrder'
  | 'statusFilter'
  | 'randomSeed'
>>;

function nonEmptyArray<T>(value: T[] | null | undefined): T[] | null {
  return value && value.length > 0 ? value : null;
}

function toSystemScopeKey(statusFilter: string | null | undefined): GridSystemScopeKey {
  switch (statusFilter) {
    case 'inbox':
      return 'inbox';
    case 'trash':
      return 'trash';
    case 'untagged':
      return 'untagged';
    case 'uncategorized':
      return 'uncategorized';
    default:
      return 'all';
  }
}

export function buildGridScopeSpec(input: ScopeQueryInput): GridScopeSpec {
  if (input.similarHashes && input.similarHashes.length > 0) {
    return {
      kind: 'similar',
      similar_hashes: input.similarHashes,
    };
  }

  const hasSmartFolder = !!(input.smartFolderPredicate && input.smartFolderPredicate.groups.length > 0);
  const rustPredicate = hasSmartFolder ? predicateToRust(input.smartFolderPredicate!) : null;

  if (input.collectionEntityId != null) {
    return {
      kind: 'collection',
      collection_entity_id: input.collectionEntityId,
    };
  }

  if (input.folderId != null) {
    return {
      kind: 'folder',
      folder_id: input.folderId,
    };
  }

  if (rustPredicate) {
    return {
      kind: 'smart',
      smart_folder_predicate: rustPredicate,
    };
  }

  return {
    kind: 'system',
    system_key: toSystemScopeKey(input.statusFilter),
  };
}

export function buildGridFilterSpec(input: FilterQueryInput): GridFilterSpec {
  return {
    search_tags: nonEmptyArray(input.searchTags),
    search_excluded_tags: nonEmptyArray(input.excludedSearchTags),
    tag_match_mode: input.tagMatchMode ?? null,
    folder_ids: input.folderId != null ? null : nonEmptyArray(input.filterFolderIds),
    excluded_folder_ids: input.folderId != null ? null : nonEmptyArray(input.excludedFilterFolderIds),
    folder_match_mode: input.folderId != null ? null : input.folderMatchMode ?? null,
    rating_min: input.ratingMin ?? null,
    mime_prefixes: nonEmptyArray(input.mimePrefixes),
    collections_only: input.collectionsOnly || null,
    color_hex: input.colorHex ?? null,
    color_accuracy: input.colorAccuracy ?? null,
    search_text: input.searchText ?? null,
  };
}

export function buildGridSortSpec(input: SortQueryInput): GridSortSpec {
  const effectiveSortField = input.smartFolderPredicate
    ? (input.smartFolderSortField ?? input.sortField)
    : input.sortField;
  const effectiveSortOrder = input.smartFolderPredicate
    ? (input.smartFolderSortOrder ?? input.sortOrder)
    : input.sortOrder;

  if (input.statusFilter === 'random') {
    return {
      field: 'random',
      order: null,
      random_seed: input.randomSeed ?? null,
    };
  }

  return {
    field: effectiveSortField ?? null,
    order: effectiveSortOrder ?? null,
    random_seed: null,
  };
}

export function buildGridQuery(input: GridQueryInput): GridQuery {
  return {
    scope: buildGridScopeSpec(input),
    filters: buildGridFilterSpec(input),
    sort: buildGridSortSpec(input),
  };
}

export function serializeGridQuery(query: GridQuery): string {
  return JSON.stringify(query);
}

export function toFetchGridPageArgs(
  query: GridQuery,
  cursor: string | null,
  limit: number,
): FetchGridPageArgs {
  return {
    limit,
    cursor,
    scope: query.scope,
    filters: query.filters,
    sort: query.sort,
  };
}
