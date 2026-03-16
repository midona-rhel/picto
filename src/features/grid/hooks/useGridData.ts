import { useCallback, useEffect, useMemo, useRef } from 'react';
import { api } from '#desktop/api';
import { prefetchMetadataBatch } from '#features/grid/data';
import type { GridRuntimeAction } from '../runtime/gridRuntimeReducer';
import type { GridRuntimeState } from '../runtime/gridRuntimeState';
import { toMasonryItem } from '../shared';
import {
  buildGridQuery,
  serializeGridQuery,
  toFetchGridPageArgs,
  type GridQuery,
  type GridQueryInput,
} from '../gridQuery';

export const PAGE_SIZE = 100;
export const MAX_LOADED_ITEMS = 10_000;

export interface GridReplacePayload {
  images: ReturnType<typeof toMasonryItem>[];
  responseTotalCount: number | null;
  hasMore: boolean;
  cursor: string | null;
  error: string | null;
}

interface UseGridDataArgs {
  queryInput: Omit<GridQueryInput, 'randomSeed'>;
  dispatch: React.Dispatch<GridRuntimeAction>;
  stateRef: { current: GridRuntimeState };
  onFirstCommit?: () => void;
}

interface UseGridDataResult {
  query: GridQuery;
  queryKey: string;
  fetchReplace: () => Promise<GridReplacePayload>;
  commitReplace: (payload: GridReplacePayload) => void;
  requestReplace: () => Promise<void>;
  requestAppend: () => Promise<void>;
}

export function useGridData({
  queryInput,
  dispatch,
  stateRef,
  onFirstCommit,
}: UseGridDataArgs): UseGridDataResult {
  const generationRef = useRef(0);
  const firstCommitDoneRef = useRef(false);
  const onFirstCommitRef = useRef(onFirstCommit);
  const randomSeedRef = useRef<number | null>(null);
  const inFlightReplaceRef = useRef<Promise<void> | null>(null);
  const inFlightReplaceKeyRef = useRef<string | null>(null);
  const queuedReplaceKeyRef = useRef<string | null>(null);

  onFirstCommitRef.current = onFirstCommit;

  if (queryInput.statusFilter === 'random' && randomSeedRef.current === null) {
    randomSeedRef.current = Math.floor(Math.random() * 0x7fffffff);
  } else if (queryInput.statusFilter !== 'random') {
    randomSeedRef.current = null;
  }

  const filterFolderIdsKey = queryInput.filterFolderIds ? JSON.stringify(queryInput.filterFolderIds) : 'null';
  const excludedFilterFolderIdsKey = queryInput.excludedFilterFolderIds ? JSON.stringify(queryInput.excludedFilterFolderIds) : 'null';
  const searchTagsKey = queryInput.searchTags ? JSON.stringify(queryInput.searchTags) : 'null';
  const excludedSearchTagsKey = queryInput.excludedSearchTags ? JSON.stringify(queryInput.excludedSearchTags) : 'null';
  const mimePrefixesKey = queryInput.mimePrefixes ? JSON.stringify(queryInput.mimePrefixes) : 'null';
  const smartFolderKey = queryInput.smartFolderPredicate ? JSON.stringify(queryInput.smartFolderPredicate) : 'null';

  const query = useMemo(() => buildGridQuery({
    ...queryInput,
    randomSeed: randomSeedRef.current,
  }), [
    queryInput.folderId,
    queryInput.collectionEntityId,
    filterFolderIdsKey,
    excludedFilterFolderIdsKey,
    queryInput.folderMatchMode,
    queryInput.statusFilter,
    searchTagsKey,
    excludedSearchTagsKey,
    queryInput.tagMatchMode,
    smartFolderKey,
    queryInput.smartFolderSortField,
    queryInput.smartFolderSortOrder,
    queryInput.sortField,
    queryInput.sortOrder,
    queryInput.ratingMin,
    mimePrefixesKey,
    queryInput.colorHex,
    queryInput.colorAccuracy,
    queryInput.searchText,
  ]);

  const queryKey = useMemo(() => serializeGridQuery(query), [query]);

  const fetchReplace = useCallback(async (): Promise<GridReplacePayload> => {
    try {
      const page = await api.grid.getPageSlim(toFetchGridPageArgs(query, null, PAGE_SIZE));
      const images = page.items.map(toMasonryItem);
      const hasMore = page.has_more && !!page.next_cursor;
      return {
        images,
        responseTotalCount: page.total_count ?? null,
        hasMore,
        cursor: page.next_cursor,
        error: null,
      };
    } catch (err) {
      return {
        images: [],
        responseTotalCount: null,
        hasMore: false,
        cursor: null,
        error: String(err),
      };
    }
  }, [query]);

  const commitReplace = useCallback((payload: GridReplacePayload) => {
    dispatch({ type: 'SET_ERROR', error: payload.error });
    if (payload.error) {
      if (!firstCommitDoneRef.current) {
        firstCommitDoneRef.current = true;
        onFirstCommitRef.current?.();
      }
      return;
    }

    dispatch({ type: 'SET_CURSOR', cursor: payload.cursor, hasMore: payload.hasMore });
    dispatch({ type: 'SET_RESPONSE_TOTAL_COUNT', count: payload.responseTotalCount });
    dispatch({ type: 'SET_IMAGES', images: payload.images });

    if (payload.images.length > 0) {
      void prefetchMetadataBatch(payload.images.map((item) => item.hash));
    }

    if (!firstCommitDoneRef.current) {
      firstCommitDoneRef.current = true;
      onFirstCommitRef.current?.();
    }
  }, [dispatch]);

  const runReplace = useCallback(async () => {
    const generation = ++generationRef.current;
    dispatch({ type: 'SET_ERROR', error: null });

    const payload = await fetchReplace();
    if (generation !== generationRef.current) return;
    commitReplace(payload);
  }, [commitReplace, dispatch, fetchReplace]);

  const requestReplace = useCallback(() => {
    if (inFlightReplaceRef.current && inFlightReplaceKeyRef.current === queryKey) {
      queuedReplaceKeyRef.current = queryKey;
      return inFlightReplaceRef.current;
    }

    inFlightReplaceKeyRef.current = queryKey;
    const requestKey = queryKey;
    const promise = runReplace().finally(() => {
      const shouldRerun = queuedReplaceKeyRef.current === requestKey;
      queuedReplaceKeyRef.current = shouldRerun ? null : queuedReplaceKeyRef.current;
      inFlightReplaceRef.current = null;
      inFlightReplaceKeyRef.current = null;
      if (shouldRerun) {
        return requestReplace();
      }
      return undefined;
    });

    inFlightReplaceRef.current = promise;
    return promise;
  }, [queryKey, runReplace]);

  const requestAppend = useCallback(async () => {
    const cursor = stateRef.current.defaultGridCursor;
    if (!cursor) {
      if (stateRef.current.hasMore) {
        dispatch({ type: 'SET_HAS_MORE', hasMore: false });
      }
      return;
    }

    const generation = generationRef.current;
    try {
      const page = await api.grid.getPageSlim(toFetchGridPageArgs(query, cursor, PAGE_SIZE));
      if (generation !== generationRef.current) return;

      const items = page.items.map(toMasonryItem);
      const nextCursor = page.next_cursor;
      const cursorAdvanced = nextCursor !== null && nextCursor !== cursor;
      const safeHasMore = page.has_more && cursorAdvanced;

      dispatch({ type: 'SET_CURSOR', cursor: nextCursor, hasMore: safeHasMore });
      dispatch({ type: 'SET_RESPONSE_TOTAL_COUNT', count: page.total_count ?? null });
      dispatch({ type: 'APPEND_IMAGES', images: items, maxItems: MAX_LOADED_ITEMS });

      if (items.length > 0) {
void prefetchMetadataBatch(items.map((item) => item.hash));
      }
    } catch (err) {
      if (generation !== generationRef.current) return;
      dispatch({ type: 'SET_ERROR', error: String(err) });
    }
  }, [dispatch, query, stateRef]);

  useEffect(() => {
    firstCommitDoneRef.current = false;
  }, [queryKey]);

  return { query, queryKey, fetchReplace, commitReplace, requestReplace, requestAppend };
}
