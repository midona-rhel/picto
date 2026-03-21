import { useCallback, useEffect, useRef } from 'react';

import type { SelectionQuerySpec } from '../metadataPrefetch';
import {
  buildGridFilterSpec,
  buildGridScopeSpec,
  buildGridSortSpec,
  type GridQueryInput,
} from '../gridQuery';
import { getOrStartSelectionSummary, pinMetadata, unpinMetadata } from '../metadataPrefetch';
import { selectedImagesPreview as selectImagesPreview, virtualSelectionSpec as selectVirtualSpec } from '../runtime';
import type { GridRuntimeAction, GridRuntimeState } from '../runtime';
import type { MasonryImageItem } from '../shared';

export type VirtualSelectionScopeInput = Partial<Pick<
  GridQueryInput,
  | 'searchTags'
  | 'excludedSearchTags'
  | 'tagMatchMode'
  | 'smartFolderPredicate'
  | 'smartFolderSortField'
  | 'smartFolderSortOrder'
  | 'sortField'
  | 'sortOrder'
  | 'statusFilter'
  | 'collectionEntityId'
  | 'folderId'
  | 'filterFolderIds'
  | 'excludedFilterFolderIds'
  | 'folderMatchMode'
  | 'randomSeed'
>> & {
  ratingMin?: number | null;
  mimePrefixes?: string[] | null;
  collectionsOnly?: boolean | null;
  colorHex?: string | null;
  colorAccuracy?: number | null;
  searchText?: string | null;
};

export interface UseGridSelectionArgs {
  state: GridRuntimeState;
  dispatch: React.Dispatch<GridRuntimeAction>;
  selectedScopeCount?: number | null;
  onSelectedImagesChange?: (images: MasonryImageItem[]) => void;
  onSelectionSummarySpecChange?: (spec: SelectionQuerySpec | null) => void;
  scope: VirtualSelectionScopeInput;
}

export interface UseGridSelectionResult {
  activateVirtualSelectAll: () => void;
}

export function buildVirtualSelectAllBaseSpec(
  scope: VirtualSelectionScopeInput,
): Omit<SelectionQuerySpec, 'excluded_hashes'> {
  return {
    mode: 'all_results',
    scope: buildGridScopeSpec(scope),
    filters: buildGridFilterSpec(scope),
    sort: buildGridSortSpec(scope),
    included_hashes: null,
    hashes: null,
  };
}

export function useGridSelection({
  state,
  dispatch,
  selectedScopeCount: _selectedScopeCount = null,
  onSelectedImagesChange,
  onSelectionSummarySpecChange,
  scope,
}: UseGridSelectionArgs): UseGridSelectionResult {
  const onSelectedImagesChangeRef = useRef(onSelectedImagesChange);
  onSelectedImagesChangeRef.current = onSelectedImagesChange;

  const activateVirtualSelectAll = useCallback(() => {
    const baseSpec = buildVirtualSelectAllBaseSpec(scope);
    dispatch({ type: 'ACTIVATE_VIRTUAL_SELECT_ALL', baseSpec });
  }, [dispatch, scope]);

  useEffect(() => {
    const preview = selectImagesPreview(state);
    onSelectedImagesChangeRef.current?.(preview);
  }, [state.selectedHashes, state.images, state.virtualAllSelection]);

  useEffect(() => {
    if (!onSelectionSummarySpecChange) return;
    const spec = selectVirtualSpec(state);
    onSelectionSummarySpecChange(spec);
  }, [state.virtualAllSelection, onSelectionSummarySpecChange]);

  useEffect(() => {
    let cancelled = false;
    if (!state.virtualAllSelection) {
      if (state.virtualAllSelectedCount !== null) {
        dispatch({ type: 'SET_VIRTUAL_ALL_COUNT', count: null });
      }
      return;
    }
    const spec = selectVirtualSpec(state);
    if (!spec) return;
    void getOrStartSelectionSummary(spec)
      .then((summary) => {
        if (!cancelled && state.virtualAllSelectedCount !== summary.selected_count) {
          dispatch({ type: 'SET_VIRTUAL_ALL_COUNT', count: summary.selected_count });
        }
      })
      .catch(() => {
        if (!cancelled && state.virtualAllSelectedCount !== null) {
          dispatch({ type: 'SET_VIRTUAL_ALL_COUNT', count: null });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [
    state.virtualAllSelection,
    state.virtualAllSelectedCount,
    dispatch,
  ]);

  const pinnedHashesRef = useRef<Set<string>>(new Set());
  useEffect(() => {
    const nextPinned = state.virtualAllSelection
      ? new Set<string>()
      : new Set(state.selectedHashes);
    for (const hash of pinnedHashesRef.current) {
      if (!nextPinned.has(hash)) unpinMetadata(hash);
    }
    for (const hash of nextPinned) {
      if (!pinnedHashesRef.current.has(hash)) pinMetadata(hash);
    }
    pinnedHashesRef.current = nextPinned;
  }, [state.selectedHashes, state.virtualAllSelection]);

  return {
    activateVirtualSelectAll,
  };
}
