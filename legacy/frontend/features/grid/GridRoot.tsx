/**
 * Grid feature root — the live entry point for the images grid view.
 *
 * Reads navigation and smart folder state directly from stores/atoms.
 * Filter derivation comes from useGridFeatureState (shared with FilterBar).
 * Selection callbacks are bridged from the MainViewModel context until
 * the inspector reads selection from Jotai atoms directly.
 */

import { useMemo, useState } from 'react';
import { useAtomValue } from 'jotai';
import { useNavigationStore } from '../../state-legacy/navigationStore';
import { folderNodesAtom, scopeCountsAtom, smartFoldersAtom, smartFolderCountsAtom } from '../../state/sidebar';
import { useMainViewSelectionState } from '../layout/components/MainViewModelContext';
import { useGridFeatureState } from './hooks/useGridFeatureState';
import { ImageGrid } from './ImageGrid';
import type { SmartFolderPredicate } from '../smart-folders/components/types';
import type { GridViewMode } from './runtime';
import type { ViewerHostController } from '../viewer/hooks/useViewerHost';

/**
 * Props from the app shell — scope-overridden preferences and
 * cross-feature concerns only.
 */
interface GridRootProps {
  externalFreeze?: boolean;
  viewer: ViewerHostController;
  viewMode: GridViewMode;
  targetSize: number;
  sortField: string;
  sortOrder: string;
  isDetailMode?: boolean;
  onViewModeChange: (mode: GridViewMode) => void;
  onSortFieldChange: (field: string) => void;
  onSortOrderChange: (order: string) => void;
}

export function GridRoot(props: GridRootProps) {
  // ── Navigation state ───────────────────────────────────────
  const currentView = useNavigationStore((s) => s.currentView);
  const activeFolderId = useNavigationStore((s) => s.activeFolderId);
  const activeCollectionId = useNavigationStore((s) => s.activeCollectionId);
  const activeSmartFolderId = useNavigationStore((s) => s.activeSmartFolderId);
  const activeStatusFilter = useNavigationStore((s) => s.activeStatusFilter);
  const similarHashes = useNavigationStore((s) => s.similarHashes);
  const filterTags = useNavigationStore((s) => s.filterTags);

  // ── Sidebar data ───────────────────────────────────────────
  const smartFolders = useAtomValue(smartFoldersAtom);
  const smartFolderCounts = useAtomValue(smartFolderCountsAtom);
  const folderNodes = useAtomValue(folderNodesAtom);
  const scopeCounts = useAtomValue(scopeCountsAtom);

  const activeSmartFolder = useMemo(() => {
    if (!activeSmartFolderId) return null;
    const sf = smartFolders.find((f) => f.id === String(activeSmartFolderId));
    if (!sf) return null;
    return {
      id: sf.id,
      name: sf.name,
      parent_id: sf.parent_id ? parseInt(sf.parent_id, 10) : null,
      icon: sf.icon,
      color: sf.color,
      predicate: sf.predicate ?? sf.localPredicate ?? { groups: [] },
      sort_field: sf.sort_field ?? null,
      sort_order: sf.sort_order ?? null,
    };
  }, [activeSmartFolderId, smartFolders]);

  // ── Filter derivation (shared with FilterBar via legacy filterStore) ──
  // Temporary bridge: selection callbacks from MainViewModel context.
  const selection = useMainViewSelectionState();
  const grid = useGridFeatureState({
    currentView,
    isDetailMode: props.isDetailMode ?? false,
    activeFolderId,
    activeCollectionId,
    activeSmartFolder,
    filterTags,
    allImagesCount: scopeCounts.active,
    activeStatusFilter,
    inboxCount: scopeCounts.inbox,
    uncategorizedCount: scopeCounts.uncategorized,
    untaggedCount: scopeCounts.untagged,
    trashCount: scopeCounts.trash,
    smartFolderCounts,
    folderNodes,
    selectedImages: [],
  });

  const [, setGridContainerWidth] = useState(0);

  return (
    <ImageGrid
      folderId={activeFolderId}
      collectionEntityId={activeCollectionId}
      statusFilter={activeStatusFilter}
      similarHashes={similarHashes}
      smartFolderPredicate={activeSmartFolder?.predicate as SmartFolderPredicate | undefined}
      smartFolderSortField={activeSmartFolder?.sort_field ?? undefined}
      smartFolderSortOrder={activeSmartFolder?.sort_order ?? undefined}
      searchTags={grid.effectiveSearchTags}
      excludedSearchTags={grid.excludedSearchTags}
      tagMatchMode={grid.tagMatchMode}
      filterFolderIds={grid.filterFolderIds}
      excludedFilterFolderIds={grid.excludedFilterFolderIds}
      folderMatchMode={grid.folderMatchMode}
      ratingMin={grid.ratingFilter}
      mimePrefixes={grid.mimePrefixes}
      collectionsOnly={grid.collectionsOnly}
      colorHex={grid.debouncedColorHex}
      colorAccuracy={grid.debouncedColorAccuracy}
      searchText={grid.searchText || grid.filterSearchText}
      viewMode={props.viewMode}
      targetSize={props.targetSize}
      sortField={props.sortField}
      sortOrder={props.sortOrder}
      onViewModeChange={props.onViewModeChange}
      onSortFieldChange={props.onSortFieldChange}
      onSortOrderChange={props.onSortOrderChange}
      onContainerWidthChange={setGridContainerWidth}
      onSelectedImagesChange={selection.onSelectedImagesChange}
      onSelectionSummarySpecChange={selection.onSelectionSummarySpecChange}
      onMediaViewStateChange={selection.onMediaViewStateChange}
      externalFreeze={props.externalFreeze}
      viewer={props.viewer}
    />
  );
}
