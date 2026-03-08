import { useCallback, useEffect, useRef } from 'react';

import { SelectionController } from '../../../shared/controllers/selectionController';
import type { SelectionQuerySpec } from '../metadataPrefetch';
import { pinMetadata, unpinMetadata } from '../metadataPrefetch';
import { selectedImagesPreview as selectImagesPreview, virtualSelectionSpec as selectVirtualSpec } from '../runtime';
import type { GridRuntimeAction, GridRuntimeState } from '../runtime';
import type { MasonryImageItem } from '../shared';

export interface VirtualSelectionScopeInput {
  searchTags?: string[];
  excludedSearchTags?: string[];
  tagMatchMode?: 'all' | 'any' | 'exact' | null;
  smartFolderPredicate?: unknown;
  smartFolderSortField?: string;
  smartFolderSortOrder?: string;
  sortField?: string;
  sortOrder?: string;
  statusFilter?: string | null;
  folderId?: number | null;
  filterFolderIds?: number[] | null;
  excludedFilterFolderIds?: number[] | null;
  folderMatchMode?: 'all' | 'any' | 'exact' | null;
}

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
  const {
    searchTags,
    excludedSearchTags,
    tagMatchMode,
    smartFolderPredicate,
    smartFolderSortField,
    smartFolderSortOrder,
    sortField,
    sortOrder,
    statusFilter,
    folderId,
    filterFolderIds,
    excludedFilterFolderIds,
    folderMatchMode,
  } = scope;

  return {
    mode: 'all_results',
    search_tags: searchTags && searchTags.length > 0 ? [...searchTags] : null,
    search_excluded_tags:
      excludedSearchTags && excludedSearchTags.length > 0 ? [...excludedSearchTags] : null,
    tag_match_mode: tagMatchMode ?? null,
    smart_folder_predicate: (smartFolderPredicate as SelectionQuerySpec['smart_folder_predicate']) ?? null,
    smart_folder_sort_field: smartFolderSortField ?? null,
    smart_folder_sort_order: smartFolderSortOrder ?? null,
    sort_field: sortField ?? null,
    sort_order: sortOrder ?? null,
    included_hashes: null,
    hashes: null,
    status: statusFilter ?? null,
    folder_ids:
      folderId != null
        ? [folderId]
        : filterFolderIds && filterFolderIds.length > 0
          ? filterFolderIds
          : null,
    excluded_folder_ids:
      folderId != null
        ? null
        : excludedFilterFolderIds && excludedFilterFolderIds.length > 0
          ? excludedFilterFolderIds
          : null,
    folder_match_mode: folderId != null ? null : folderMatchMode ?? null,
  };
}

export function useGridSelection({
  state,
  dispatch,
  selectedScopeCount = null,
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
    if (
      selectedScopeCount != null &&
      state.virtualAllSelectedCount !== selectedScopeCount
    ) {
      dispatch({ type: 'SET_VIRTUAL_ALL_COUNT', count: selectedScopeCount });
    }
    const spec = selectVirtualSpec(state);
    if (!spec) return;
    void SelectionController.getOrStartSummary(spec)
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
    selectedScopeCount,
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
