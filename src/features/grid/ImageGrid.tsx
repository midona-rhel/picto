import { useCallback, useEffect, useLayoutEffect, useRef, useMemo, useState } from 'react';
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
import { useDomainStore } from '../../state/domainStore';
import type { MediaViewState, MediaViewControls } from '../../features/viewer/hooks/useViewerHost';
import type { ViewerHostController } from '../../features/viewer/hooks/useViewerHost';
import { useGridData } from './hooks/useGridData';
import { useGridHotkeys } from './hooks/useGridHotkeys';
import { useGridKeyboardNavigation } from './hooks/useGridKeyboardNavigation';
import { useGridContextMenu } from './hooks/useGridContextMenu';
import { useGridSelection } from './hooks/useGridSelection';
import { useGridMarqueeSelection } from './hooks/useGridMarqueeSelection';
import { useGridInlineRename } from './hooks/useGridInlineRename';
import { useGridExportActions } from './hooks/useGridExportActions';
import { useGridImportActions } from './hooks/useGridImportActions';
import { useGridViewerSource } from './hooks/useGridViewerSource';
import { useGridLiveInsertion } from './hooks/useGridLiveInsertion';
import { useGridRefreshLifecycle } from './hooks/useGridRefreshLifecycle';
import { useGridSwapController } from './hooks/useGridSwapController';
import { useGridDisplayState } from './hooks/useGridDisplayState';
import { useGridActionHandlers } from './hooks/useGridActionHandlers';
import { resolveGridEmptyContext } from './gridEmptyContext';
import { buildGridSurfaceModel } from './gridSurfaceModel';
import { ThumbnailPipeline } from '../../shared/lib/canvas/thumbnailPipeline';

interface GridScrollShellProps {
  scrollRef: React.MutableRefObject<HTMLDivElement | null>;
  onContextMenu: React.MouseEventHandler<HTMLDivElement>;
  onPointerDown: React.PointerEventHandler<HTMLDivElement>;
  grayscalePreview: boolean;
  gridFreezeActive: boolean;
  children: React.ReactNode;
}

function GridScrollShell({
  scrollRef,
  onContextMenu,
  onPointerDown,
  grayscalePreview,
  gridFreezeActive,
  children,
}: GridScrollShellProps) {
  const handleShellRef = useCallback((node: HTMLDivElement | null) => {
    scrollRef.current = node;
  }, [scrollRef]);

  return (
    <div
      ref={handleShellRef}
      data-grid-container
      onContextMenu={onContextMenu}
      onPointerDown={onPointerDown}
      style={{
        flex: 1,
        overflowY: 'auto',
        scrollbarGutter: 'stable both-edges',
        overflowX: 'hidden',
        overflowAnchor: 'none',
        userSelect: 'none',
        WebkitUserSelect: 'none',
        position: 'relative',
        pointerEvents: gridFreezeActive ? 'none' : 'auto',
        filter: grayscalePreview ? 'grayscale(1)' : undefined,
      } as React.CSSProperties}
    >
      {children}
    </div>
  );
}

function hasVisibleSubfoldersForFolder(
  folderNodes: Array<{ parent_id: string | null }>,
  folderId: number | null,
  showSubfolders: boolean,
): boolean {
  if (!folderId || !showSubfolders) return false;
  const parentNodeId = `folder:${folderId}`;
  return folderNodes.some((n) => n.parent_id === parentNodeId);
}

function easeOutCubic(t: number): number {
  return 1 - Math.pow(1 - t, 3);
}



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
  const selectedHashesRef = useRef(state.selectedHashes);
  selectedHashesRef.current = state.selectedHashes;
  const dismissHoverPreviewRef = useRef<() => void>(() => {});
  const dismissVideoScrubRef = useRef<() => void>(() => {});
  const sharedThumbnailAtlasRef = useRef<ThumbnailPipeline | null>(null);
  if (!sharedThumbnailAtlasRef.current) {
    sharedThumbnailAtlasRef.current = new ThumbnailPipeline();
  }

  const contextMenu = useContextMenu();
  const navigateToFolder = useNavigationStore(s => s.navigateToFolder);
  const navigateToCollection = useNavigationStore(s => s.navigateToCollection);
  const saveScrollTop = useNavigationStore(s => s.saveScrollTop);
  const saveLoadedItemCount = useNavigationStore(s => s.saveLoadedItemCount);
  const saveRandomSeed = useNavigationStore(s => s.saveRandomSeed);
  const historyIndex = useNavigationStore(s => s.historyIndex);
  const consumeScrollRestore = useNavigationStore(s => s.consumeScrollRestore);
  const {
    displaySettings,
    displaySettingsRef,
    hasVisibleSubfolders,
    updateSetting,
  } = useGridDisplayState({
    displayFolderId: state.displayFolderId,
  });
  const folderNodes = useDomainStore((s) => s.folderNodes);
  // Track whether the first load has completed so we don't show "No images"
  // while the DB query is still in flight.
  const initialLoadDone = useRef(false);
  const displayViewModeRef = useRef(state.displayViewMode);
  displayViewModeRef.current = state.displayViewMode;
  const animatedTargetSizeRef = useRef(targetSize);
  const [animatedTargetSize, setAnimatedTargetSize] = useState(targetSize);

  useEffect(() => {
    const start = animatedTargetSizeRef.current;
    const end = targetSize;
    const delta = end - start;
    if (Math.abs(delta) < 1) {
      animatedTargetSizeRef.current = end;
      setAnimatedTargetSize(end);
      return;
    }

    if (Math.abs(delta) < 16) {
      animatedTargetSizeRef.current = end;
      setAnimatedTargetSize(end);
      return;
    }

    const durationMs = Math.min(180, Math.max(110, Math.abs(delta) * 0.9));
    let rafId = 0;
    let animationStart = 0;

    const tick = (ts: number) => {
      if (!animationStart) animationStart = ts;
      const elapsed = ts - animationStart;
      const progress = Math.min(1, elapsed / durationMs);
      const next = start + delta * easeOutCubic(progress);
      animatedTargetSizeRef.current = next;
      setAnimatedTargetSize(next);
      if (progress < 1) {
        rafId = requestAnimationFrame(tick);
      }
    };

    rafId = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(rafId);
  }, [targetSize]);

  useEffect(() => {
    dispatch({
      type: 'COMMIT_GEOMETRY',
      viewMode,
      targetSize: animatedTargetSize,
      folderId: folderId ?? null,
      searchTags,
      emptyContext: resolveGridEmptyContext(smartFolderPredicate, folderId, statusFilter),
    });
  }, [animatedTargetSize, dispatch, folderId, searchTags, smartFolderPredicate, statusFilter, viewMode]);
  const {
    queryKey,
    fetchReplace,
    commitReplace,
    requestReplace,
    requestAppend,
    randomSeed,
  } = useGridData({
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
    restoredRandomSeed: useNavigationStore.getState().pendingRandomSeed,
  });

  const gap = 8;
  const activeGridImages = state.images;
  const navigationScopeKey = useMemo(() => JSON.stringify({
    collectionEntityId: collectionEntityId ?? null,
    folderId: folderId ?? null,
    searchTags: searchTags ?? [],
    excludedSearchTags: excludedSearchTags ?? [],
    tagMatchMode: tagMatchMode ?? null,
    smartFolderPredicate: smartFolderPredicate ? JSON.stringify(smartFolderPredicate) : null,
    filterFolderIds: filterFolderIds ?? [],
    excludedFilterFolderIds: excludedFilterFolderIds ?? [],
    folderMatchMode: folderMatchMode ?? null,
    statusFilter: statusFilter ?? null,
  }), [
    collectionEntityId,
    excludedFilterFolderIds,
    excludedSearchTags,
    filterFolderIds,
    folderId,
    folderMatchMode,
    searchTags,
    smartFolderPredicate,
    statusFilter,
    tagMatchMode,
  ]);

  // Reset grayscale preview when navigating to a different scope
  useEffect(() => {
    if (displaySettings.grayscalePreview) {
      updateSetting('grayscalePreview', false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [navigationScopeKey]);

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
      searchTags: searchTags ?? null,
      excludedSearchTags: excludedSearchTags ?? null,
      tagMatchMode: tagMatchMode ?? null,
      smartFolderPredicate: smartFolderPredicate ?? null,
      smartFolderSortField: smartFolderSortField ?? null,
      smartFolderSortOrder: smartFolderSortOrder ?? null,
      sortField,
      sortOrder,
      randomSeed,
      statusFilter: statusFilter ?? null,
      collectionEntityId: collectionEntityId ?? null,
      folderId: folderId ?? null,
      filterFolderIds: filterFolderIds ?? null,
      excludedFilterFolderIds: excludedFilterFolderIds ?? null,
      folderMatchMode: folderMatchMode ?? null,
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
    selectedHashesRef,
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
    dismissHoverPreviewRef,
    dismissVideoScrubRef,
  });

  const {
    dialogOpen: exportDialogOpen,
    setDialogOpen: setExportDialogOpen,
    dialogState: exportDialogState,
    setDialogState: setExportDialogState,
    canConfirmAdvancedExport,
    selectOutputDir,
    handleBasicExport,
    openAdvancedExport,
    handleConfirmAdvancedExport,
  } = useGridExportActions({
    stateRef,
    selectedScopeCount,
  });

  useGridHotkeys({
    stateRef,
    dispatch,
    activateVirtualSelectAll,
    handleOpenWithDefaultApp,
    handleRevealInFolder,
    handleOpenInNewWindow,
    handleBasicExport,
    handleAdvancedExport: openAdvancedExport,
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
  const liveSurface = useMemo(() => buildGridSurfaceModel({
    scopeKey: navigationScopeKey,
    images: activeGridImages,
    responseTotalCount: state.responseTotalCount,
    totalCount: state.responseTotalCount,
    hasMore: state.hasMore,
    displayViewMode: state.displayViewMode,
    displayTargetSize: state.displayTargetSize,
    displayFolderId: state.displayFolderId,
    displaySearchTags: state.displaySearchTags,
    displayEmptyContext: state.displayEmptyContext,
    selectedSubfolderId: state.selectedSubfolderId,
    showEmptyState: initialLoadDone.current && !hasVisibleSubfolders,
  }), [
    activeGridImages,
    hasVisibleSubfolders,
    navigationScopeKey,
    selectedScopeCount,
    state.displayEmptyContext,
    state.displayFolderId,
    state.displaySearchTags,
    state.displayTargetSize,
    state.displayViewMode,
    state.hasMore,
    state.responseTotalCount,
    state.selectedSubfolderId,
  ]);

  const buildCommittedSurface = useCallback((payload: Awaited<ReturnType<typeof fetchReplace>>, scopeChanged: boolean) => {
    const nextFolderId = folderId ?? null;
    const nextHasVisibleSubfolders = hasVisibleSubfoldersForFolder(
      folderNodes,
      nextFolderId,
      displaySettings.showSubfolders,
    );
    return buildGridSurfaceModel({
      scopeKey: navigationScopeKey,
      images: payload.images,
      responseTotalCount: payload.responseTotalCount,
      totalCount: payload.responseTotalCount,
      hasMore: payload.hasMore,
      displayViewMode: viewMode,
      displayTargetSize: animatedTargetSizeRef.current,
      displayFolderId: nextFolderId,
      displaySearchTags: searchTags,
      displayEmptyContext: resolveGridEmptyContext(smartFolderPredicate, folderId, statusFilter),
      selectedSubfolderId: scopeChanged ? null : stateRef.current.selectedSubfolderId,
      showEmptyState: !payload.error && payload.images.length === 0 && !nextHasVisibleSubfolders,
    });
  }, [
    displaySettings.showSubfolders,
    folderId,
    folderNodes,
    navigationScopeKey,
    searchTags,
    smartFolderPredicate,
    stateRef,
    statusFilter,
    targetSize,
    viewMode,
  ]);

  const {
    renderedScopeKey,
    renderedSurface,
    preserveScrollBehaviors,
    visibleTransitionStage,
  } = useGridSwapController({
    incomingScopeKey: navigationScopeKey,
    queryKey,
    liveSurface,
    viewMode,
    targetSize,
    folderId,
    searchTags,
    smartFolderPredicate,
    statusFilter,
    fetchReplace,
    commitReplace,
    buildCommittedSurface,
    initialLoadDone,
    viewer,
    dispatch,
    onMediaViewStateChange,
    consumeScrollRestore,
    scrollRef,
  });
  imagesRef.current = renderedSurface.images;
  displayViewModeRef.current = renderedSurface.displayViewMode;

  useGridRefreshLifecycle({
    dispatch,
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
    refreshTrigger,
    stateRef,
    requestReplace,
  });
  const gridFreezeActive = externalFreeze;

  useEffect(() => {
    return () => {
      sharedThumbnailAtlasRef.current?.destroy();
      sharedThumbnailAtlasRef.current = null;
    };
  }, []);

  // Continuously save scroll position to navigation history (debounced).
  // This ensures the position is always current when back/forward triggers.
  useEffect(() => {
    const scrollEl = scrollRef.current;
    if (!scrollEl) return;
    let timer: ReturnType<typeof setTimeout>;
    const onScroll = () => {
      if (visibleTransitionStage !== 'idle') {
        clearTimeout(timer);
        return;
      }
      const currentHistoryIndex = historyIndex;
      clearTimeout(timer);
      timer = setTimeout(() => {
        if (visibleTransitionStage !== 'idle') return;
        saveScrollTop(scrollEl.scrollTop, currentHistoryIndex);
        saveLoadedItemCount(imagesRef.current.length);
        if (randomSeed != null) saveRandomSeed(randomSeed);
      }, 150);
    };
    scrollEl.addEventListener('scroll', onScroll, { passive: true });
    return () => {
      scrollEl.removeEventListener('scroll', onScroll);
      clearTimeout(timer);
    };
  }, [historyIndex, imagesRef, randomSeed, renderedScopeKey, saveLoadedItemCount, saveRandomSeed, saveScrollTop, scrollRef, visibleTransitionStage]);

  useGridViewerSource({
    viewer,
    images: renderedSurface.images,
    totalCount: renderedSurface.totalCount,
    statusFilter,
    handleInboxAction: statusFilter === 'inbox' ? handleInboxAction : undefined,
    onMediaViewStateChange,
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
  });

  if (state.error) {
    return <GridErrorState error={state.error} onRetry={requestReplace} />;
  }

  return (
    <div
      key={renderedScopeKey}
      style={{
        height: '100%',
        display: 'flex',
        position: 'relative',
        opacity: transitionOpacity(visibleTransitionStage),
        transition: transitionCss(visibleTransitionStage),
        visibility: visibleTransitionStage === 'preparing' ? 'hidden' : 'visible',
      }}
    >
      <GridScrollShell
        scrollRef={scrollRef}
        onContextMenu={handleContextMenu}
        onPointerDown={handleBoxPointerDown}
        grayscalePreview={displaySettings.grayscalePreview}
        gridFreezeActive={gridFreezeActive}
      >
        <div style={{ position: 'relative' }}>
          {renderedSurface.displayFolderId != null && displaySettings.showSubfolders ? (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              <SubfolderGrid
                folderId={renderedSurface.displayFolderId}
                targetSize={renderedSurface.displayTargetSize}
                totalImageCount={renderedSurface.images.length}
                onOpenFolder={(id, name) => navigateToFolder({ folder_id: id, name })}
                selectedSubfolderId={renderedSurface.selectedSubfolderId}
                paused={gridFreezeActive}
                onSelectedSubfolderChange={(id) => {
                  dispatch({ type: 'SET_SELECTED_SUBFOLDER', id });
                  dispatch({ type: 'SELECT_HASHES', hashes: new Set() });
                }}
              />
              <CanvasGrid
                images={renderedSurface.images}
                targetSize={renderedSurface.displayTargetSize}
                gap={gap}
                viewMode={renderedSurface.displayViewMode}
                selectedHashes={effectiveSelectedHashes}
                searchTags={renderedSurface.displaySearchTags}
                onImageClick={handleImageClick}
                onImport={handleImport}
                onImportFolder={handleImportFolderRequest}
                onContainerWidthChange={handleContainerWidthChange}
                showEmptyState={renderedSurface.showEmptyState}
                emptyContext={renderedSurface.displayEmptyContext}
                scrollContainerRef={scrollRef}
                popHash={state.popHash}
                onPopComplete={() => dispatch({ type: 'SET_POP_HASH', hash: null })}
                frozen={gridFreezeActive || isTransitionFrozen(visibleTransitionStage)}
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
                onLoadMore={renderedSurface.hasMore ? loadMore : undefined}
                totalCount={renderedSurface.totalCount}
                renamingHash={renamingHash}
                scrollAnchorScopeKey={renderedScopeKey}
                preserveScrollBehaviors={preserveScrollBehaviors}
                topInset={0}
                atlasRef={sharedThumbnailAtlasRef}
                dismissHoverPreviewRef={dismissHoverPreviewRef}
                dismissVideoScrubRef={dismissVideoScrubRef}
              />
            </div>
          ) : (
            <CanvasGrid
              images={renderedSurface.images}
              targetSize={renderedSurface.displayTargetSize}
              gap={gap}
              viewMode={renderedSurface.displayViewMode}
              selectedHashes={effectiveSelectedHashes}
              searchTags={renderedSurface.displaySearchTags}
              onImageClick={handleImageClick}
              onImport={handleImport}
              onImportFolder={handleImportFolderRequest}
              onContainerWidthChange={handleContainerWidthChange}
              showEmptyState={renderedSurface.showEmptyState}
              emptyContext={renderedSurface.displayEmptyContext}
              scrollContainerRef={scrollRef}
              popHash={state.popHash}
              onPopComplete={() => dispatch({ type: 'SET_POP_HASH', hash: null })}
              frozen={gridFreezeActive || isTransitionFrozen(visibleTransitionStage)}
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
              onLoadMore={renderedSurface.hasMore ? loadMore : undefined}
              totalCount={renderedSurface.totalCount}
              renamingHash={renamingHash}
              scrollAnchorScopeKey={renderedScopeKey}
              preserveScrollBehaviors={preserveScrollBehaviors}
              topInset={8}
              atlasRef={sharedThumbnailAtlasRef}
            />
          )}
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
      </GridScrollShell>

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
        exportDialogOpen={exportDialogOpen}
        exportDialogState={exportDialogState}
        onCloseExportDialog={() => setExportDialogOpen(false)}
        onExportDialogChange={(patch) => setExportDialogState((current) => ({ ...current, ...patch }))}
        onChooseExportDir={selectOutputDir}
        onConfirmExport={handleConfirmAdvancedExport}
        canConfirmExport={canConfirmAdvancedExport}
      />
    </div>
  );
}
