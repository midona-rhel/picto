import { useCallback, useEffect, useMemo, useRef } from 'react';
import type { GridRuntimeAction } from '../runtime/gridRuntimeReducer';
import type { GridRuntimeState } from '../runtime/gridRuntimeState';
import type { MasonryImageItem } from '../shared';
import type { GridQueryKey } from './gridQueryKey';
import { GridQueryBroker } from './GridQueryBroker';
import type { SmartFolderPredicate } from '../../../features/smart-folders/components/types';
import { predicateToRust } from '../../../features/smart-folders/components/types';

// ---------------------------------------------------------------------------
// Props accepted by the hook (subset of ImageGridProps relevant to querying)
// ---------------------------------------------------------------------------

export interface GridQueryBrokerProps {
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
}

// ---------------------------------------------------------------------------
// Hook
// ---------------------------------------------------------------------------

export function useGridQueryBroker(
  props: GridQueryBrokerProps,
  dispatch: React.Dispatch<GridRuntimeAction>,
  stateRef: { current: GridRuntimeState },
  viewModeRef: { current: string },
  prewarmFn: ((items: MasonryImageItem[]) => Promise<void>) | null,
  onFirstCommit: (() => void) | null,
  onEstimateSampleChanged: ((items: MasonryImageItem[]) => void) | null,
): {
  broker: GridQueryBroker;
  queryKey: GridQueryKey;
  requestReplace: () => void;
  requestAppend: () => Promise<void>;
} {
  // --- Broker instance: stable across renders ---
  const brokerRef = useRef<GridQueryBroker | null>(null);
  if (!brokerRef.current) {
    brokerRef.current = new GridQueryBroker();
  }
  const broker = brokerRef.current;

  // --- Build canonical query key ---
  const {
    folderId, collectionEntityId, filterFolderIds, excludedFilterFolderIds, folderMatchMode, statusFilter, searchTags, excludedSearchTags, tagMatchMode,
    smartFolderPredicate, smartFolderSortField, smartFolderSortOrder,
    sortField, sortOrder, ratingMin, mimePrefixes,
    colorHex, colorAccuracy, searchText,
  } = props;

  // Smart folder sort override
  const effectiveSortField = smartFolderPredicate
    ? (smartFolderSortField ?? sortField)
    : sortField;
  const effectiveSortOrder = smartFolderPredicate
    ? (smartFolderSortOrder ?? sortOrder)
    : sortOrder;

  // Convert predicate for the key (null if empty or absent)
  const hasSmartFolder = smartFolderPredicate && smartFolderPredicate.groups.length > 0;
  const rustPredicate = hasSmartFolder ? predicateToRust(smartFolderPredicate!) : null;

  // Stable serialization deps for useMemo
  const filterFolderIdsKey = filterFolderIds ? JSON.stringify(filterFolderIds) : 'null';
  const excludedFilterFolderIdsKey = excludedFilterFolderIds ? JSON.stringify(excludedFilterFolderIds) : 'null';
  const searchTagsKey = searchTags ? JSON.stringify(searchTags) : 'null';
  const excludedSearchTagsKey = excludedSearchTags ? JSON.stringify(excludedSearchTags) : 'null';
  const smartFolderKey = rustPredicate ? JSON.stringify(rustPredicate) : 'null';
  const mimePrefixesKey = mimePrefixes ? JSON.stringify(mimePrefixes) : 'null';

  // Seeded random: generate a stable seed when entering random view, clear on exit
  const randomSeedRef = useRef<number | null>(null);
  if (statusFilter === 'random' && randomSeedRef.current === null) {
    randomSeedRef.current = Math.floor(Math.random() * 0x7FFFFFFF);
  } else if (statusFilter !== 'random') {
    randomSeedRef.current = null;
  }

  const queryKey: GridQueryKey = useMemo(() => ({
    folderId: folderId ?? null,
    collectionEntityId: collectionEntityId ?? null,
    filterFolderIds: filterFolderIds ?? null,
    excludedFilterFolderIds: excludedFilterFolderIds ?? null,
    folderMatchMode: folderMatchMode ?? null,
    statusFilter: statusFilter ?? null,
    searchTags: searchTags && searchTags.length > 0 ? searchTags : null,
    excludedSearchTags: excludedSearchTags && excludedSearchTags.length > 0 ? excludedSearchTags : null,
    tagMatchMode: tagMatchMode ?? null,
    smartFolderPredicate: rustPredicate,
    sortField: effectiveSortField,
    sortOrder: effectiveSortOrder,
    ratingMin: ratingMin ?? null,
    mimePrefixes: mimePrefixes ?? null,
    colorHex: colorHex ?? null,
    colorAccuracy: colorAccuracy ?? null,
    searchText: searchText ?? null,
    randomSeed: randomSeedRef.current,
  }), [
    folderId, collectionEntityId, filterFolderIdsKey, excludedFilterFolderIdsKey, folderMatchMode, statusFilter, searchTagsKey, excludedSearchTagsKey, tagMatchMode,
    smartFolderKey, effectiveSortField, effectiveSortOrder,
    ratingMin, mimePrefixesKey, colorHex, colorAccuracy, searchText,
  ]);

  // --- Wire broker each render (cheap ref assignments) ---
  broker.wire(dispatch, stateRef, viewModeRef, prewarmFn, onFirstCommit, onEstimateSampleChanged);

  // --- Stable key ref for callbacks ---
  const queryKeyRef = useRef(queryKey);
  queryKeyRef.current = queryKey;

  // --- Stable callbacks ---
  const requestReplace = useCallback(() => {
    broker.requestReplace(queryKeyRef.current);
  }, [broker]);

  const requestAppend = useCallback(async () => {
    await broker.requestAppend(queryKeyRef.current);
  }, [broker]);

  // --- Cleanup ---
  useEffect(() => () => broker.destroy(), [broker]);

  return { broker, queryKey, requestReplace, requestAppend };
}
