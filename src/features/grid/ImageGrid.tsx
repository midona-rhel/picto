import { useLayoutEffect, useRef, useMemo, useState } from 'react';
import { useGridRuntime } from './runtime';
import { effectiveSelectedHashes as selectEffectiveHashes } from './runtime';
import {
  type GridViewMode,
} from './runtime';
import { useContextMenu } from '../../shared/components/ContextMenu';
import { imageDrag } from '../../shared/lib/imageDrag';
import { type MediaItem } from './shared';
import {
  type SelectionQuerySpec,
} from './metadataPrefetch';
import type { SmartFolderPredicate } from '../../features/smart-folders/components/types';
import { CanvasGrid } from './CanvasGrid';
import { SubfolderGrid } from '../folders/components/SubfolderGrid';
import { GridInlineRenameOverlay } from './components/GridInlineRenameOverlay';
import { transitionOpacity, transitionCss, isTransitionFrozen } from './runtime/gridTransitionPipeline';
import { GridDialogsLayer } from './components/GridDialogsLayer';
import { GridErrorState } from './components/GridErrorState';
import { useNavigationStore } from '../../state/navigationStore';
import type { MediaViewState, MediaViewControls } from '../../features/viewer/hooks/useViewerHost';
import type { ViewerHostController } from '../../features/viewer/hooks/useViewerHost';
import { useGridData } from './hooks/useGridData';
import { useGridHotkeys } from './hooks/useGridHotkeys';
import { useGridKeyboardNavigation } from './hooks/useGridKeyboardNavigation';
import { useGridContextMenu } from './hooks/useGridContextMenu';
import { useGridSelection } from './hooks/useGridSelection';
import { useGridMarqueeSelection } from './hooks/useGridMarqueeSelection';
import { useGridInlineRename } from './hooks/useGridInlineRename';
import { useGridImportActions } from './hooks/useGridImportActions';
import { useGridViewerSource } from './hooks/useGridViewerSource';
import { useGridLiveInsertion } from './hooks/useGridLiveInsertion';
import { useGridRefreshLifecycle } from './hooks/useGridRefreshLifecycle';
import { useGridDisplayState } from './hooks/useGridDisplayState';
import { useGridActionHandlers } from './hooks/useGridActionHandlers';
import { resolveGridEmptyContext } from './gridEmptyContext';

// Re-export GridViewMode from runtime for backward compatibility
export type { GridViewMode } from './runtime';

interface ImageGridProps {
  searchTags?: string[];
  excludedSearchTags?: string[];
  tagMatchMode?: 'all' | 'any' | 'exact' | null;
  smartFolderPredicate?: SmartFolderPredicate;
  smartFolderSortField?: string;
  smartFolderSortOrder?: string;
  folderId?: number | null;
  collectionEntityId?: number | null;
  /** Filter bar folder IDs — narrows results to specific folders */
  filterFolderIds?: number[] | null;
  /** Filter bar excluded folder IDs */
  excludedFilterFolderIds?: number[] | null;
  /** Include-folder matching mode */
  folderMatchMode?: 'all' | 'any' | 'exact' | null;
  /** Explicit status filter (e.g. 'trash' for status=2 files) */
  statusFilter?: string | null;
  viewMode?: GridViewMode;
  targetSize?: number;
  onViewModeChange?: (mode: GridViewMode) => void;
  sortField?: string;
  sortOrder?: string;
  onSortFieldChange?: (field: string) => void;
  onSortOrderChange?: (order: string) => void;
  onContainerWidthChange?: (width: number) => void;
  refreshTrigger?: number;
  onSelectedImagesChange?: (images: MediaItem[]) => void;
  onSelectionSummarySpecChange?: (spec: SelectionQuerySpec | null) => void;
  selectedScopeCount?: number | null;
  onMediaViewStateChange?: (state: MediaViewState | null, controls: MediaViewControls | null) => void;
  // Filter bar props
  ratingMin?: number | null;
  mimePrefixes?: string[] | null;
  colorHex?: string | null;
  colorAccuracy?: number | null;
  searchText?: string;
  externalFreeze?: boolean;
  viewer: ViewerHostController;
}

export function ImageGrid({ searchTags, excludedSearchTags, tagMatchMode, smartFolderPredicate, smartFolderSortField, smartFolderSortOrder, folderId, collectionEntityId, filterFolderIds, excludedFilterFolderIds, folderMatchMode, statusFilter, viewMode = 'waterfall', targetSize = 250, onViewModeChange, sortField = 'imported_at', sortOrder = 'asc', onSortFieldChange, onSortOrderChange, onContainerWidthChange, refreshTrigger, onSelectedImagesChange, onSelectionSummarySpecChange, selectedScopeCount = null, onMediaViewStateChange, ratingMin, mimePrefixes, colorHex, colorAccuracy, searchText, externalFreeze = false, viewer }: ImageGridProps) {
  const { state, dispatch } = useGridRuntime({
    viewMode,
    targetSize,
    folderId: folderId ?? null,
    searchTags,
    emptyContext: resolveGridEmptyContext(smartFolderPredicate, folderId, statusFilter),
  });

  // Stable ref to latest state for use in callbacks (avoids stale closures)
  const stateRef = useRef(state);
  stateRef.current = state;

  const contextMenu = useContextMenu();
  const navigateToFolder = useNavigationStore(s => s.navigateToFolder);
  const navigateToCollection = useNavigationStore(s => s.navigateToCollection);
  const {
    displaySettings,
    displaySettingsRef,
    hasVisibleSubfolders,
    updateSetting,
  } = useGridDisplayState({
    displayFolderId: state.displayFolderId,
  });
  // Track whether the first load has completed so we don't show "No images"
  // while the DB query is still in flight.
  const initialLoadDone = useRef(false);
  const displayViewModeRef = useRef(state.displayViewMode);
  displayViewModeRef.current = state.displayViewMode;
  const { queryKey, requestReplace, requestAppend } = useGridData({
    queryInput: {
      folderId: folderId ?? null,
      collectionEntityId: collectionEntityId ?? null,
      filterFolderIds: filterFolderIds ?? null,
      excludedFilterFolderIds: excludedFilterFolderIds ?? null,
      folderMatchMode: folderMatchMode ?? null,
      statusFilter: statusFilter ?? null,
      searchTags: searchTags ?? null,
      excludedSearchTags: excludedSearchTags ?? null,
      tagMatchMode: tagMatchMode ?? null,
      smartFolderPredicate: smartFolderPredicate ?? null,
      smartFolderSortField: smartFolderSortField ?? null,
      smartFolderSortOrder: smartFolderSortOrder ?? null,
      sortField,
      sortOrder,
      ratingMin: ratingMin ?? null,
      mimePrefixes: mimePrefixes ?? null,
      colorHex: colorHex ?? null,
      colorAccuracy: colorAccuracy ?? null,
      searchText: searchText || null,
    },
    dispatch,
    stateRef,
    onFirstCommit: () => { initialLoadDone.current = true; },
  });

  const gap = 8;
  const activeGridImages = state.images;
  const resolvedGridTotalCount = state.responseTotalCount ?? selectedScopeCount ?? activeGridImages.length;
  // Refs for values that change frequently but shouldn't invalidate handleImageClick
  const imagesRef = useRef(activeGridImages);
  imagesRef.current = activeGridImages;
  const lastClickedHashRef = useRef(state.lastClickedHash);
  lastClickedHashRef.current = state.lastClickedHash;

  const effectiveSelectedHashes = useMemo(
    () => selectEffectiveHashes(state),
    [state.images, state.selectedHashes, state.virtualAllSelection],
  );

  // Keep imageDrag module-level ref in sync so tiles can read it without a prop
  useLayoutEffect(() => {
    imageDrag.setSelectedHashes(effectiveSelectedHashes);
  }, [effectiveSelectedHashes]);

  const { activateVirtualSelectAll } = useGridSelection({
    state,
    dispatch,
    selectedScopeCount,
    onSelectedImagesChange,
    onSelectionSummarySpecChange,
    scope: {
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
    },
  });

  const singleSelectedHash = !state.virtualAllSelection && state.selectedHashes.size === 1
    ? [...state.selectedHashes][0]
    : null;

  const [batchRenameOpen, setBatchRenameOpen] = useState(false);
  const {
    renamingHash,
    renameValue,
    renameInputRef,
    renameCancelledRef,
    setRenameValue,
    setRenamingHash,
    startInlineRename,
    commitRename,
    cancelRename,
  } = useGridInlineRename({
    singleSelectedHash,
    stateRef,
    requestGridReload: requestReplace,
  });

  const {
    scrollRef,
    getCanvasOffsetTop,
    handleContainerWidthChange,
    scrollToIndex,
    handleGridNavigation,
  } = useGridKeyboardNavigation({
    stateRef,
    imagesRef,
    lastClickedHashRef,
    displayViewModeRef,
    displaySettingsRef,
    gap,
    dispatch,
    onContainerWidthChange,
  });

  const {
    handleBoxPointerDown,
    marqueeRectRef,
    marqueeHitHashesRef,
    scheduleRedrawRef,
    canvasLayoutRef,
  } = useGridMarqueeSelection({
    boxActive: state.boxActive,
    dispatch,
    scrollRef,
    getCanvasOffsetTop,
    imagesRef,
  });

  const {
    handleDeleteSelected,
    handleRateSelected,
    handleRestoreSelected,
    handleInboxAction,
    handleInboxSelectionAction,
    handleRemoveFromFolder,
    handleRemoveFromCollection,
    handleOpenDetail,
    handleOpenQuickLook,
    handleOpenWithDefaultApp,
    handleOpenInNewWindow,
    handleRevealInFolder,
    handleCopyFilePath,
    handleCopyTags,
    handlePasteTags,
    hasCopiedTags,
    recordImageView,
    handleImageClick,
    isReorderScope,
    handleReorder,
    loadMore,
  } = useGridActionHandlers({
    state,
    stateRef,
    dispatch,
    imagesRef,
    lastClickedHashRef,
    canvasLayoutRef,
    viewer,
    selectedScopeCount,
    statusFilter,
    folderId,
    collectionEntityId,
    requestReplace,
    requestAppend,
    displayFolderId: state.displayFolderId,
  });

  useGridHotkeys({
    stateRef,
    dispatch,
    activateVirtualSelectAll,
    handleOpenWithDefaultApp,
    handleRevealInFolder,
    handleOpenInNewWindow,
    handleDeleteSelected,
    handleCopyFilePath,
    handleCopyTags,
    handlePasteTags,
    onViewModeChange,
    updateSetting,
    grayscalePreview: displaySettings.grayscalePreview,
    openSlideshow: viewer.openSlideshow,
    setBatchRenameOpen,
    startInlineRename,
    folderId,
    collectionEntityId,
    handleRemoveFromFolder,
    handleRemoveFromCollection,
    handleGridNavigation,
    handleRateSelected,
    handleOpenQuickLook,
    handleOpenDetail,
    viewerOpen: viewer.isOpen,
    closeViewer: viewer.close,
    statusFilter,
  });

  const folderIdRef = useRef(folderId);
  folderIdRef.current = folderId;
  const {
    folderImportDialog,
    setFolderImportDialog,
    handleImport,
    handleImportFolderRequest,
    handleConfirmImportFolder,
  } = useGridImportActions({
    folderIdRef,
    requestGridReload: requestReplace,
    setDragOver: (over) => dispatch({ type: 'SET_DRAG_OVER', over }),
  });

  useGridLiveInsertion({
    dispatch,
    stateRef,
    sortField,
    sortOrder,
    folderId,
    collectionEntityId,
    smartFolderPredicate,
    searchTags,
    excludedSearchTags,
    filterFolderIds,
    excludedFilterFolderIds,
    ratingMin,
    mimePrefixes,
    colorHex,
    searchText,
    statusFilter,
  });
  const gridFreezeActive = externalFreeze;
  useGridRefreshLifecycle({
    dispatch,
    viewMode,
    targetSize,
    folderId,
    collectionEntityId,
    searchTags,
    excludedSearchTags,
    tagMatchMode,
    smartFolderPredicate,
    filterFolderIds,
    excludedFilterFolderIds,
    folderMatchMode,
    statusFilter,
    queryKey,
    requestReplace,
    refreshTrigger,
    stateRef,
    initialLoadDone,
    viewer,
    onMediaViewStateChange,
    scrollRef,
  });

  useGridViewerSource({
    viewer,
    images: activeGridImages,
    totalCount: resolvedGridTotalCount,
    statusFilter,
    handleInboxAction: statusFilter === 'inbox' ? handleInboxAction : undefined,
    onMediaViewStateChange,
    recordImageView,
    dispatch,
    scrollToIndex,
    imagesRef,
  });

  const handleContextMenu = useGridContextMenu({
    scrollRef,
    getCanvasOffsetTop,
    canvasLayoutRef,
    imagesRef,
    state,
    stateRef,
    effectiveSelectedHashes,
    dispatch,
    viewMode,
    onViewModeChange,
    sortField,
    sortOrder,
    onSortFieldChange,
    onSortOrderChange,
    smartFolderPredicate,
    smartFolderSortField,
    smartFolderSortOrder,
    folderId,
    statusFilter,
    contextMenu,
    activateVirtualSelectAll,
    handleDeleteSelected,
    handleRestoreSelected,
    handleRemoveFromFolder,
    handleRemoveFromCollection,
    handleInboxAction,
    handleInboxSelectionAction,
    handleCopyTags,
    handlePasteTags,
    hasCopiedTags,
    handleOpenDetail: viewer.openDetail,
    collectionEntityId,
    navigateToCollection,
    setRenameValue,
    setRenamingHash,
    renameCancelledRef,
    setBatchRenameOpen,
    requestGridReload: () => { void requestReplace(); },
  });

  if (state.error) {
    return <GridErrorState error={state.error} onRetry={requestReplace} />;
  }

  return (
    <div style={{ height: '100%', display: 'flex', position: 'relative' }}>
      <div
        ref={scrollRef as React.RefObject<HTMLDivElement>}
        data-grid-container
        onContextMenu={handleContextMenu}
        onPointerDown={handleBoxPointerDown}
        style={{
          flex: 1,
          overflowY: 'auto',
          scrollbarGutter: 'stable both-edges',
          overflowX: 'hidden',
          userSelect: 'none',
          WebkitUserSelect: 'none',
          position: 'relative',
          pointerEvents: gridFreezeActive ? 'none' : 'auto',
          filter: displaySettings.grayscalePreview ? 'grayscale(1)' : undefined,
          opacity: transitionOpacity(state.transitionStage),
          transition: transitionCss(state.transitionStage),
        } as React.CSSProperties}
      >
        <div style={{ height: 8 }} />
        <div style={{ position: 'relative' }}>
          {state.displayFolderId != null && displaySettings.showSubfolders && (
            <SubfolderGrid
              folderId={state.displayFolderId}
              targetSize={state.displayTargetSize}
              totalImageCount={activeGridImages.length}
              onOpenFolder={(id, name) => navigateToFolder({ folder_id: id, name })}
              selectedSubfolderId={state.selectedSubfolderId}
              paused={gridFreezeActive}
              onSelectedSubfolderChange={(id) => {
                dispatch({ type: 'SET_SELECTED_SUBFOLDER', id });
                dispatch({ type: 'SELECT_HASHES', hashes: new Set() });
              }}
            />
          )}
          <CanvasGrid
            images={activeGridImages}
            targetSize={state.displayTargetSize}
            gap={gap}
            viewMode={state.displayViewMode}
            selectedHashes={effectiveSelectedHashes}
            searchTags={state.displaySearchTags}
            onImageClick={handleImageClick}
            onImport={handleImport}
            onImportFolder={handleImportFolderRequest}
            onContainerWidthChange={handleContainerWidthChange}
            showEmptyState={initialLoadDone.current && !hasVisibleSubfolders}
            emptyContext={state.displayEmptyContext}
            scrollContainerRef={scrollRef}
            popHash={state.popHash}
            onPopComplete={() => dispatch({ type: 'SET_POP_HASH', hash: null })}
            frozen={gridFreezeActive || isTransitionFrozen(state.transitionStage)}
            marqueeActive={state.boxActive}
            showTileName={displaySettings.showTileName}
            showResolution={displaySettings.showResolution}
            showExtension={displaySettings.showExtension}
            showExtensionLabel={displaySettings.showExtensionLabel}
            thumbnailFitMode={displaySettings.thumbnailFitMode}
            marqueeRectRef={marqueeRectRef}
            marqueeHitHashesRef={marqueeHitHashesRef}
            scheduleRedrawRef={scheduleRedrawRef}
            onLayoutChange={(positions) => { canvasLayoutRef.current = positions; }}
            reorderMode={isReorderScope}
            onReorder={isReorderScope ? handleReorder : undefined}
            onLoadMore={state.hasMore ? loadMore : undefined}
            totalCount={resolvedGridTotalCount}
            renamingHash={renamingHash}
          />
          {renamingHash && (
            <GridInlineRenameOverlay
              renamingHash={renamingHash}
              positions={canvasLayoutRef.current}
              images={imagesRef.current}
              showTileName={displaySettings.showTileName}
              showResolution={displaySettings.showResolution}
              scrollRoot={scrollRef.current}
              renameInputRef={renameInputRef}
              renameValue={renameValue}
              setRenameValue={setRenameValue}
              commitRename={commitRename}
              cancelRename={cancelRename}
            />
          )}
        </div>
      </div>

      <GridDialogsLayer
        contextMenuState={contextMenu.state}
        onCloseContextMenu={contextMenu.close}
        isDragOver={state.isDragOver}
        batchRenameOpen={batchRenameOpen}
        onCloseBatchRename={() => setBatchRenameOpen(false)}
        batchRenameImages={
          state.virtualAllSelection
            ? activeGridImages.filter(i => !state.virtualAllSelection!.excludedHashes.has(i.hash))
            : activeGridImages.filter(i => state.selectedHashes.has(i.hash))
        }
        folderImportDialog={folderImportDialog}
        setFolderImportDialog={setFolderImportDialog}
        onConfirmImportFolder={handleConfirmImportFolder}
      />
    </div>
  );
}
