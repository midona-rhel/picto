import { useEffect, useLayoutEffect, useCallback, useRef, useMemo, useState } from 'react';
import { useGridRuntime } from './runtime';
import { effectiveSelectedHashes as selectEffectiveHashes } from './runtime';
import {
  type GridViewMode,
  type GridEmptyContext,
} from './runtime';
import { TextButton } from '../../shared/components/TextButton';
import { ConfirmModal } from '../../shared/components/ConfirmModal';
import { StateBlock, StateActions } from '../../shared/components/state';
import { notifyError } from '../../shared/lib/notify';
import { registerUndoAction } from '../../shared/controllers/undoRedoController';
import { api, getCurrentWebview, listenRuntimeEvent, open } from '#desktop/api';
import { ContextMenu, useContextMenu } from '../../shared/components/ContextMenu';
import { imageDrag } from '../../shared/lib/imageDrag';
import { sortLiveImages } from './liveSort';
import { toMasonryItem, type MediaItem } from './shared';
import {
  prefetchMetadata,
  type SelectionQuerySpec,
} from './metadataPrefetch';
import { VirtualGrid as DomGridSurface } from './VirtualGrid';
import type { SmartFolderPredicate } from '../../features/smart-folders/components/types';
import type { DragDropPayload, FolderReorderMove } from '../../shared/types/api';
import { useImportActionStore } from '../../state/importActionStore';
import { useGridMetadataStore } from '../../state/gridMetadataStore';
import { useManualImportStore } from '../../state/manualImportStore';
import { useSettingsStore } from '../../state/settingsStore';
import { useScopedDisplay } from '../../shared/contexts/ScopedDisplayContext';
import { BatchRenameDialog } from '../../features/grid/components/BatchRenameDialog';
import { useDomainStore } from '../../state/domainStore';
import { useNavigationStore } from '../../state/navigationStore';
import { SubfolderGrid } from './SubfolderGrid';
import type { MediaViewState, MediaViewControls } from '../../features/viewer/hooks/useViewerHost';
import type { ViewerHostController } from '../../features/viewer/hooks/useViewerHost';
import { useGridData } from './hooks/useGridData';
import { useGridMutationActions } from './hooks/useGridMutationActions';
import { useGridHotkeys } from './hooks/useGridHotkeys';
import { useGridItemActions } from './hooks/useGridItemActions';
import { useGridKeyboardNavigation } from './hooks/useGridKeyboardNavigation';
import { useGridContextMenu } from './hooks/useGridContextMenu';
import { useGridSelection } from './hooks/useGridSelection';
import { useGridMarqueeSelection } from './hooks/useGridMarqueeSelection';
import type { FileImportedEvent } from '../../shared/types/api/events';

// Re-export GridViewMode from runtime for backward compatibility
export type { GridViewMode } from './runtime';

function resolveGridEmptyContext(
  smartFolderPredicate: SmartFolderPredicate | null | undefined,
  folderId: number | null | undefined,
  statusFilter: string | null | undefined,
): GridEmptyContext {
  if (smartFolderPredicate) return 'smart-folder';
  if (folderId) return 'folder';
  if (statusFilter === 'inbox') return 'inbox';
  if (statusFilter === 'uncategorized') return 'uncategorized';
  if (statusFilter === 'untagged') return 'untagged';
  return 'default';
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

  const contextMenu = useContextMenu();
  const { settings: globalSettings, updateSetting } = useSettingsStore();
  const scopedCtx = useScopedDisplay();
  const scopedOpts = scopedCtx?.displayOptions;
  const displaySettings = useMemo(() => ({
    ...globalSettings,
    ...(scopedOpts ? {
      showTileName: scopedOpts.showTileName,
      showResolution: scopedOpts.showResolution,
      showExtension: scopedOpts.showExtension,
      showExtensionLabel: scopedOpts.showExtensionLabel,
      thumbnailFitMode: scopedOpts.thumbnailFitMode,
    } : {}),
  }), [globalSettings, scopedOpts]);
  const navigateToFolder = useNavigationStore(s => s.navigateToFolder);
  const navigateToCollection = useNavigationStore(s => s.navigateToCollection);
  const folderNodes = useDomainStore(s => s.folderNodes);
  const hasVisibleSubfolders = useMemo(() => {
    if (!state.displayFolderId || !displaySettings.showSubfolders) return false;
    const parentNodeId = `folder:${state.displayFolderId}`;
    return folderNodes.some(n => n.parent_id === parentNodeId);
  }, [state.displayFolderId, folderNodes, displaySettings.showSubfolders]);
  // Track whether the first load has completed so we don't show "No images"
  // while the DB query is still in flight.
  const initialLoadDone = useRef(false);
  const displayViewModeRef = useRef(state.displayViewMode);
  displayViewModeRef.current = state.displayViewMode;
  const { queryKey, outlineTotalCount, requestReplace } = useGridData({
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
    onFirstCommit: () => { initialLoadDone.current = true; },
  });

  const gap = 8;
  const activeGridImages = state.images;
  const resolvedGridTotalCount = outlineTotalCount ?? selectedScopeCount ?? activeGridImages.length;
  const renderCommitKey = `${queryKey}|${activeGridImages.length}`;

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

  const {
    handleDeleteSelected,
    handleRateSelected,
    handleRestoreSelected,
    handleInboxAction,
    handleInboxSelectionAction,
    handleRemoveFromFolder,
    handleRemoveFromCollection,
  } = useGridMutationActions({
    stateRef,
    dispatch,
    statusFilter,
    folderId,
    collectionEntityId,
    requestGridReload: () => { void requestReplace(); },
  });

  // Helper: get the single selected hash (for actions that require exactly one)
  const singleSelectedHash = !state.virtualAllSelection && state.selectedHashes.size === 1
    ? [...state.selectedHashes][0]
    : null;

  const [batchRenameOpen, setBatchRenameOpen] = useState(false);

  const [renamingHash, setRenamingHash] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState('');
  const renameInputRef = useRef<HTMLInputElement>(null);
  const renamingHashRef = useRef(renamingHash);
  renamingHashRef.current = renamingHash;

  // Focus + select when rename starts
  useEffect(() => {
    if (!renamingHash) return;
    // Retry focus until input is mounted (layout may update)
    let attempts = 0;
    const tryFocus = () => {
      if (renameInputRef.current) {
        renameInputRef.current.focus();
        renameInputRef.current.select();
      } else if (attempts < 10) {
        attempts++;
        setTimeout(tryFocus, 30);
      }
    };
    setTimeout(tryFocus, 0);
  }, [renamingHash]);

  const startInlineRename = useCallback(() => {
    if (!singleSelectedHash) return;
    const img = stateRef.current.images.find(i => i.hash === singleSelectedHash);
    renameCancelledRef.current = false;
    setRenameValue(img?.name ?? '');
    setRenamingHash(singleSelectedHash);
  }, [singleSelectedHash]);

  const commitRename = useCallback(() => {
    if (renameCancelledRef.current) return; // Escape already cancelled
    const hash = renamingHashRef.current;
    if (!hash) return;
    const img = stateRef.current.images.find(i => i.hash === hash);
    const before = img?.name || null;
    const after = renameValue.trim() || null;
    setRenamingHash(null);
    if (after === before) return;
    api.files.setName(hash, after)
      .then(() => {
        registerUndoAction({
          label: 'Rename file',
          undo: () => api.files.setName(hash, before),
          redo: () => api.files.setName(hash, after),
        });
        void requestReplace();
      })
      .catch(err => notifyError(err, 'Rename Failed'));
  }, [renameValue, requestReplace]);

  const renameCancelledRef = useRef(false);
  const cancelRename = useCallback(() => {
    renameCancelledRef.current = true;
    setRenamingHash(null);
  }, []);

  // Cancel rename if selection changes away from the renaming file
  useEffect(() => {
    if (renamingHash && singleSelectedHash !== renamingHash) {
      setRenamingHash(null);
    }
  }, [singleSelectedHash, renamingHash]);

  const {
    handleOpenDetail,
    handleOpenQuickLook,
    handleOpenWithDefaultApp,
    handleOpenInNewWindow,
    handleRevealInFolder,
    handleCopyFilePath,
    handleCopyTags,
    handlePasteTags,
    hasCopiedTags,
  } = useGridItemActions({
    state,
    stateRef,
    imagesRef,
    singleSelectedHash,
    viewer,
    selectedScopeCount,
  });

  const recordImageView = useCallback((hash: string) => {
    const image = stateRef.current.images.find((img) => img.hash === hash);
    if (!image || image.is_collection) return;
    void api.files
      .incrementViewCount(hash)
      
      .catch((err) => {
        console.warn('Failed to increment view count:', err);
      });
  }, [stateRef]);

  const displaySettingsRef = useRef(displaySettings);
  displaySettingsRef.current = displaySettings;
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

  const handleImageClick = useCallback((image: MediaItem, event: React.MouseEvent) => {
    if (event.detail === 2) {
      viewer.openDetail(image.hash);
      return;
    }
    // Prefetch metadata at click time so the properties panel has it instantly
    prefetchMetadata(image.hash);
    const { virtualAllSelection } = stateRef.current;
    if (virtualAllSelection) {
      if (event.metaKey || event.ctrlKey) {
        dispatch({ type: 'TOGGLE_VIRTUAL_EXCLUSION', hash: image.hash });
        dispatch({ type: 'SET_LAST_CLICKED', hash: image.hash });
        return;
      }
      // Plain click exits virtual select-all and selects a single item.
      dispatch({ type: 'DEACTIVATE_VIRTUAL_SELECT_ALL' });
    }
    if (event.metaKey || event.ctrlKey) {
      dispatch({ type: 'TOGGLE_HASH', hash: image.hash });
    } else if (event.shiftKey && lastClickedHashRef.current) {
      // Use layout positions for visual order (correct for all layouts including waterfall)
      const positions = canvasLayoutRef.current;
      const currentImages = imagesRef.current;
      const prevSelected = stateRef.current.selectedHashes;
      if (positions.length > 0) {
        // Build index sorted by visual position (y, then x)
        const indices = Array.from({ length: Math.min(positions.length, currentImages.length) }, (_, i) => i);
        indices.sort((a, b) => {
          const pa = positions[a];
          const pb = positions[b];
          const dy = pa.y - pb.y;
          if (Math.abs(dy) > pa.h * 0.5) return dy;
          return pa.x - pb.x;
        });
        const visualHashes = indices.map(i => currentImages[i].hash);
        const startIdx = visualHashes.indexOf(lastClickedHashRef.current!);
        const endIdx = visualHashes.indexOf(image.hash);
        if (startIdx !== -1 && endIdx !== -1) {
          const [lo, hi] = [Math.min(startIdx, endIdx), Math.max(startIdx, endIdx)];
          const next = new Set(prevSelected);
          for (let i = lo; i <= hi; i++) next.add(visualHashes[i]);
          dispatch({ type: 'SELECT_HASHES', hashes: next });
          dispatch({ type: 'SET_LAST_CLICKED', hash: image.hash });
          return;
        }
      }
      // Fallback to array order
      const startIdx = currentImages.findIndex(i => i.hash === lastClickedHashRef.current);
      const endIdx = currentImages.findIndex(i => i.hash === image.hash);
      const [lo, hi] = [Math.min(startIdx, endIdx), Math.max(startIdx, endIdx)];
      const next = new Set(prevSelected);
      for (let i = lo; i <= hi; i++) next.add(currentImages[i].hash);
      dispatch({ type: 'SELECT_HASHES', hashes: next });
    } else {
      dispatch({ type: 'SELECT_HASHES', hashes: new Set([image.hash]) });
    }
    dispatch({ type: 'SET_LAST_CLICKED', hash: image.hash });
  }, [dispatch, viewer]);

  const isReorderScope = !!state.displayFolderId || !!collectionEntityId;

  const handleReorder = useCallback((movedHashes: string[], targetIndex: number) => {
    if (!folderId && !collectionEntityId) return;
    const currentFolderId = folderId ?? null;
    const currentCollectionId = collectionEntityId ?? null;
    const prev = stateRef.current.images;

    const movedSet = new Set(movedHashes);
    const remaining = prev.filter(img => !movedSet.has(img.hash));
    const movedItems = movedHashes
      .map(h => prev.find(img => img.hash === h))
      .filter(Boolean) as MediaItem[];

    const movedBefore = prev.slice(0, targetIndex).filter(img => movedSet.has(img.hash)).length;
    const insertAt = Math.max(0, Math.min(remaining.length, targetIndex - movedBefore));

    const next = [...remaining];
    next.splice(insertAt, 0, ...movedItems);

    if (currentCollectionId != null) {
      dispatch({ type: 'SET_IMAGES', images: next });
      api.collections.reorderMembers(currentCollectionId, next.map((img) => img.hash)).catch(err => {
        console.error('Collection reorder failed, reloading collection:', err);
        void requestReplace();
      });
      return;
    }

    const moves: FolderReorderMove[] = [];
    for (let i = 0; i < movedItems.length; i++) {
      const pos = insertAt + i;
      if (i === 0) {
        if (pos > 0) {
          moves.push({ hash: movedItems[i].hash, after_hash: next[pos - 1].hash, before_hash: null });
        } else if (next.length > movedItems.length) {
          moves.push({ hash: movedItems[i].hash, after_hash: null, before_hash: next[movedItems.length].hash });
        }
      } else {
        moves.push({ hash: movedItems[i].hash, after_hash: movedItems[i - 1].hash, before_hash: null });
      }
    }

    dispatch({ type: 'SET_IMAGES', images: next });

    if (moves.length > 0) {
      api.folders.reorderItems(currentFolderId!, moves).catch(err => {
        console.error('Reorder failed, reloading folder:', err);
        void requestReplace();
      });
    }
  }, [folderId, collectionEntityId, dispatch, requestReplace]);

  const folderIdRef = useRef(folderId);
  folderIdRef.current = folderId;
  const [folderImportDialog, setFolderImportDialog] = useState<{ path: string; preserveStructure: boolean } | null>(null);

  const importRequestToken = useImportActionStore((s) => s.requestToken);
  const importHandledToken = useImportActionStore((s) => s.handledToken);
  const importRequestKind = useImportActionStore((s) => s.requestKind);
  const markImportHandled = useImportActionStore((s) => s.markHandled);

  const handleImport = useCallback(async () => {
    try {
      const selected = await open({
        multiple: true,
        filters: [{
          name: 'Images',
          extensions: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'tiff', 'svg', 'mp4', 'webm', 'mov', 'mkv', 'avi'],
        }],
      });
      if (!selected) return;
      const paths = Array.isArray(selected) ? selected : [selected];
      const store = useManualImportStore.getState();
      const currentFolderId = folderIdRef.current;
      let imported = 0;
      let skipped = 0;
      let errors = 0;
      store.start(paths.length);

      for (let index = 0; index < paths.length; index += 1) {
        const result = await api.import.files([paths[index]]);
        const importedHashes = result.imported.map((file) => file.hash);
        if (currentFolderId != null && importedHashes.length > 0) {
          await api.folders.addFiles(currentFolderId, importedHashes);
        }
        imported += result.imported.length;
        skipped += result.skipped.length;
        errors += result.errors.length;
        store.setProgress({
          done: index + 1,
          total: paths.length,
          imported,
          skipped,
          errors,
        });
        await requestReplace();
      }

      store.finish({ imported, skipped, errors });
    } catch (err) {
      useManualImportStore.getState().fail();
      notifyError(err, 'Import Failed');
    }
  }, [requestReplace]);

  const handleImportFolderRequest = useCallback(async () => {
    const selected = await open({
      properties: ['openDirectory'],
      message: 'Select a folder to import',
    });
    const pickedPath = Array.isArray(selected) ? selected[0] : selected;
    if (!pickedPath) return;
    setFolderImportDialog({
      path: pickedPath,
      preserveStructure: true,
    });
  }, []);

  const handleConfirmImportFolder = useCallback(async () => {
    const pendingImport = folderImportDialog;
    if (!pendingImport) return;
    setFolderImportDialog(null);
    try {
      const store = useManualImportStore.getState();
      store.startBackend('Adding folder');
      const result = await api.import.folder(
        pendingImport.path,
        pendingImport.preserveStructure,
        folderIdRef.current ?? null,
      );
      await requestReplace();
      store.finish({
        imported: result.imported.length,
        skipped: result.skipped.length,
        errors: result.errors.length,
      });
    } catch (err) {
      useManualImportStore.getState().fail();
      notifyError(err, 'Import Folder Failed');
    }
  }, [folderImportDialog, requestReplace]);

  useEffect(() => {
    if (importRequestToken === importHandledToken) return;
    markImportHandled(importRequestToken);
    if (importRequestKind === 'folder') {
      void handleImportFolderRequest();
      return;
    }
    void handleImport();
  }, [
    handleImport,
    handleImportFolderRequest,
    importHandledToken,
    importRequestKind,
    importRequestToken,
    markImportHandled,
  ]);

  useEffect(() => {
    const unlisten = listenRuntimeEvent('file-imported', (event: FileImportedEvent) => {
      if (folderId != null || collectionEntityId != null || smartFolderPredicate) return;
      if (searchTags?.length || excludedSearchTags?.length || filterFolderIds?.length || excludedFilterFolderIds?.length) return;
      if (ratingMin != null || mimePrefixes?.length || colorHex || searchText) return;
      if (statusFilter === 'trash' || statusFilter === 'untagged' || statusFilter === 'uncategorized' || statusFilter === 'recently_viewed') return;
      if (statusFilter === 'inbox' && event.status !== 'inbox') return;
      if ((statusFilter == null || statusFilter === 'active') && event.status !== 'active') return;

      const nextItem = toMasonryItem({
        ...event,
        name: event.name ?? null,
        width: event.width ?? null,
        height: event.height ?? null,
        duration_ms: event.duration_ms ?? null,
        num_frames: event.num_frames ?? null,
        rating: event.rating ?? null,
        source_urls: null,
      });
      const currentImages = stateRef.current.images;
      if (currentImages.some((image) => image.hash === nextItem.hash)) return;

      const nextImages = sortLiveImages(
        [...currentImages, nextItem],
        sortField,
        sortOrder as 'asc' | 'desc',
      );
      dispatch({ type: 'SET_IMAGES', images: nextImages });
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [
    collectionEntityId,
    colorHex,
    dispatch,
    excludedFilterFolderIds,
    excludedSearchTags,
    filterFolderIds,
    folderId,
    mimePrefixes,
    ratingMin,
    searchTags,
    searchText,
    smartFolderPredicate,
    sortField,
    sortOrder,
    statusFilter,
  ]);


  const handleViewerDetailImageChange = useCallback((hash: string) => {
    recordImageView(hash);
    dispatch({ type: 'SELECT_HASHES', hashes: new Set([hash]) });
    dispatch({ type: 'SET_LAST_CLICKED', hash });
  }, [dispatch, recordImageView]);

  const handleViewerQuickLookOpen = useCallback((hash: string) => {
    recordImageView(hash);
  }, [recordImageView]);

  const handleViewerQuickLookImageChange = useCallback((hash: string) => {
    recordImageView(hash);
    dispatch({ type: 'SELECT_HASHES', hashes: new Set([hash]) });
    dispatch({ type: 'SET_LAST_CLICKED', hash });
    const idx = imagesRef.current.findIndex(i => i.hash === hash);
    if (idx >= 0) scrollToIndex(idx);
  }, [dispatch, recordImageView, scrollToIndex]);

  const handleViewerCloseDetail = useCallback((exitHash: string) => {
    if (!exitHash) return;
    dispatch({ type: 'SELECT_HASHES', hashes: new Set([exitHash]) });
    dispatch({ type: 'SET_LAST_CLICKED', hash: exitHash });
  }, [dispatch]);

  const handleViewerCloseQuickLook = useCallback(() => {
    // Quick Look closing no longer drives pop animation in the grid.
  }, []);

  useEffect(() => {
    viewer.registerSource({
      images: activeGridImages,
      totalCount: resolvedGridTotalCount,
      inboxMode: statusFilter === 'inbox',
      onInboxAction: statusFilter === 'inbox' ? handleInboxAction : undefined,
      onDetailStateChange: onMediaViewStateChange,
      onDetailImageChange: handleViewerDetailImageChange,
      onQuickLookOpen: handleViewerQuickLookOpen,
      onQuickLookImageChange: handleViewerQuickLookImageChange,
      onCloseDetail: handleViewerCloseDetail,
      onCloseQuickLook: handleViewerCloseQuickLook,
    });
  }, [
    viewer,
    activeGridImages,
    resolvedGridTotalCount,
    statusFilter,
    handleInboxAction,
    onMediaViewStateChange,
    handleViewerDetailImageChange,
    handleViewerQuickLookOpen,
    handleViewerQuickLookImageChange,
    handleViewerCloseDetail,
    handleViewerCloseQuickLook,
  ]);

  const gridFreezeActive = externalFreeze;

  const scopeKey = useMemo(() => JSON.stringify({
    searchTags: searchTags ?? [],
    excludedSearchTags: excludedSearchTags ?? [],
    tagMatchMode: tagMatchMode ?? null,
    smartFolderPredicate: smartFolderPredicate ? JSON.stringify(smartFolderPredicate) : null,
    folderId: folderId ?? null,
    collectionEntityId: collectionEntityId ?? null,
    filterFolderIds: filterFolderIds ?? [],
    excludedFilterFolderIds: excludedFilterFolderIds ?? [],
    folderMatchMode: folderMatchMode ?? null,
    statusFilter: statusFilter ?? null,
  }), [
    searchTags,
    excludedSearchTags,
    tagMatchMode,
    smartFolderPredicate,
    folderId,
    collectionEntityId,
    filterFolderIds,
    excludedFilterFolderIds,
    folderMatchMode,
    statusFilter,
  ]);

  useEffect(() => {
    dispatch({
      type: 'COMMIT_GEOMETRY',
      viewMode,
      targetSize,
      folderId: folderId ?? null,
      searchTags,
      emptyContext: resolveGridEmptyContext(smartFolderPredicate, folderId, statusFilter),
    });
  }, [dispatch, viewMode, targetSize, folderId, searchTags, smartFolderPredicate, statusFilter]);

  useEffect(() => {
    viewer.close('');
    onMediaViewStateChange?.(null, null);
    dispatch({ type: 'CLEAR_SELECTION' });
    dispatch({ type: 'SET_SELECTED_SUBFOLDER', id: null });
  }, [dispatch, onMediaViewStateChange, scopeKey, viewer]);

  useEffect(() => {
    initialLoadDone.current = false;
    void requestReplace();
  }, [queryKey, requestReplace]);

  // Background refresh from subscriptions
  const prevRefreshTrigger = useRef(refreshTrigger);
  useEffect(() => {
    if (prevRefreshTrigger.current !== refreshTrigger) {
      prevRefreshTrigger.current = refreshTrigger;
      void requestReplace();
    }
  }, [refreshTrigger, requestReplace]);

  // Optimistic grid removal — inspector enqueues hashes when removing from active folder.
  // Also handles detail view: images array shrinks → MediaView auto-advances.
  const pendingGridRemovals = useGridMetadataStore((s) => s.pendingGridRemovals);
  useEffect(() => {
    if (pendingGridRemovals.size === 0) return;
    const toRemove = new Set(pendingGridRemovals);
    useGridMetadataStore.getState().clearGridRemovals();
    dispatch({ type: 'FILTER_IMAGES', predicate: img => !toRemove.has(img.hash) });
    dispatch({ type: 'REMOVE_HASHES', hashes: toRemove });
  }, [pendingGridRemovals, dispatch]);

  // Set active grid scope for scope-aware invalidation filtering in gridRefresher
  useEffect(() => {
    let scope: string;
    if (collectionEntityId != null) scope = `collection:${collectionEntityId}`;
    else if (folderId != null) scope = `folder:${folderId}`;
    else if (statusFilter === 'inbox') scope = 'system:inbox';
    else if (statusFilter === 'trash') scope = 'system:trash';
    else scope = 'system:all';
    useGridMetadataStore.getState().setActiveGridScope(scope);
  }, [folderId, collectionEntityId, statusFilter]);

  // Patch grid tiles in-place when metadata changes (name, rating, etc.) without full reload
  const metadataInvalidatedHashes = useGridMetadataStore((s) => s.metadataInvalidatedHashes);
  useEffect(() => {
    if (metadataInvalidatedHashes.size === 0) return;
    const hashes = [...metadataInvalidatedHashes];
    useGridMetadataStore.getState().clearInvalidatedHashes();

    useGridMetadataStore.getState().fetchMetadataBatch(hashes).then((results) => {
      if (results.length === 0) return;
      const metaMap = new Map(results.map(r => [r.file.hash, r.file]));
      const currentImages = stateRef.current.images;
      let changed = false;
      const next = currentImages.map(img => {
        const meta = metaMap.get(img.hash);
        if (!meta) return img;
        if (img.name === meta.name && img.rating === meta.rating && img.view_count === meta.view_count) return img;
        changed = true;
        return { ...img, name: meta.name, rating: meta.rating, view_count: meta.view_count };
      });
      if (changed) dispatch({ type: 'SET_IMAGES', images: next });
    });
  }, [metadataInvalidatedHashes]);

  // Reload grid when gridRefresher bumps gridRefreshSeq (mutation with grid_scopes)
  const gridRefreshSeq = useGridMetadataStore((s) => s.gridRefreshSeq);
  const prevGridRefreshSeq = useRef(gridRefreshSeq);
  useEffect(() => {
    if (prevGridRefreshSeq.current !== gridRefreshSeq) {
      prevGridRefreshSeq.current = gridRefreshSeq;
      void requestReplace();
    }
  }, [gridRefreshSeq, requestReplace]);

  useEffect(() => {
    const webview = getCurrentWebview();
    const promise = webview.onDragDropEvent(async (event) => {
      const payload = event.payload as DragDropPayload;
      if (payload.type === 'enter') {
        // Never show import overlay for internal native drags.
        const pendingInternalHashes = imageDrag.getPendingNativeDragHashes();
        dispatch({ type: 'SET_DRAG_OVER', over: !pendingInternalHashes });
      } else if (payload.type === 'leave') {
        dispatch({ type: 'SET_DRAG_OVER', over: false });
      } else if (payload.type === 'drop') {
        dispatch({ type: 'SET_DRAG_OVER', over: false });
        // PBI-053: Idempotent clear of native drag session.
        const pendingHashes = imageDrag.getPendingNativeDragHashes();
        imageDrag.clearNativeDragSession();

        // Skip import for internal drags (files from our blob store)
        if (pendingHashes) return;

        const paths = payload.paths;
        if (paths.length === 0) return;
        try {
          const store = useManualImportStore.getState();
          const currentFolderId = folderIdRef.current;
          let imported = 0;
          let skipped = 0;
          let errors = 0;
          store.start(paths.length);

          for (let index = 0; index < paths.length; index += 1) {
            const result = await api.import.files([paths[index]]);
            const importedHashes = result.imported.map((file) => file.hash);
            if (currentFolderId != null && importedHashes.length > 0) {
              await api.folders.addFiles(currentFolderId, importedHashes);
            }
            imported += result.imported.length;
            skipped += result.skipped.length;
            errors += result.errors.length;
            store.setProgress({
              done: index + 1,
              total: paths.length,
              imported,
              skipped,
              errors,
            });
            await requestReplace();
          }

          store.finish({ imported, skipped, errors });
        } catch (err) {
          useManualImportStore.getState().fail();
          notifyError(err, 'Import Failed');
        }
      }
    });
    return () => { promise.then((unlisten) => unlisten()); };
  }, []);

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
    return (
      <div style={{ height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        <StateBlock
          variant="error"
          title="Failed to load images"
          description={state.error}
          action={(
            <StateActions>
              <TextButton onClick={requestReplace}>Retry</TextButton>
            </StateActions>
          )}
        />
      </div>
    );
  }

  return (
    <div style={{ height: '100%', display: 'flex', position: 'relative' }}>
      {/* Grid area — kept mounted but hidden when detail view is active to preserve scroll position */}
      <div
        ref={scrollRef}
        data-grid-container
        onContextMenu={handleContextMenu}
        onPointerDown={handleBoxPointerDown}
        style={{
          flex: 1,
          // Reserve scrollbar gutter on both sides for symmetric padding.
          overflowY: 'auto',
          scrollbarGutter: 'stable both-edges',
          overflowX: 'hidden',
          userSelect: 'none',
          WebkitUserSelect: 'none',
          position: 'relative',
          pointerEvents: gridFreezeActive ? 'none' : 'auto',
          filter: displaySettings.grayscalePreview ? 'grayscale(1)' : undefined,
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
          <DomGridSurface
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
            frozen={gridFreezeActive}
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
            renamingHash={renamingHash}
            renameValue={renameValue}
            renameInputRef={renameInputRef}
            onRenameChange={setRenameValue}
            onRenameCommit={commitRename}
            onRenameCancel={cancelRename}
            renderCommitKey={renderCommitKey}
            datasetKey={queryKey}
          />
        </div>
      </div>

      {contextMenu.state && (
        <ContextMenu
          items={contextMenu.state.items}
          position={contextMenu.state.position}
          onClose={contextMenu.close}
        />
      )}

      {state.isDragOver && (
        <div
          style={{
            position: 'absolute',
            zIndex: 1002,
            top: 0,
            left: 0,
            right: 0,
            bottom: 0,
            boxSizing: 'border-box',
            border: '2px solid var(--color-primary)',
            backgroundColor: 'var(--color-primary-10, rgba(59, 130, 246, 0.1))',
            borderRadius: 8,
            cursor: 'copy',
            pointerEvents: 'none',
          }}
        >
          <div
            style={{
              position: 'absolute',
              bottom: 16,
              left: '50%',
              width: 200,
              marginLeft: -100,
              padding: 12,
              textAlign: 'center',
              color: 'var(--color-white-99)',
              fontSize: 'var(--font-size-md)',
              fontWeight: 'var(--font-weight-bold)',
              background: 'var(--color-primary)',
              lineHeight: 'var(--line-height-relaxed)',
              borderRadius: 6,
              pointerEvents: 'none',
              animation: 'pulse 0.8s infinite',
            }}
          >
            Drop files to import
          </div>
        </div>
      )}

      <BatchRenameDialog
        opened={batchRenameOpen}
        onClose={() => setBatchRenameOpen(false)}
        images={
          state.virtualAllSelection
            ? activeGridImages.filter(i => !state.virtualAllSelection!.excludedHashes.has(i.hash))
            : activeGridImages.filter(i => state.selectedHashes.has(i.hash))
        }
      />

      <ConfirmModal
        opened={folderImportDialog != null}
        onClose={() => setFolderImportDialog(null)}
        onConfirm={() => { void handleConfirmImportFolder(); }}
        title="Import Folder"
        confirmLabel="Import"
      >
        <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          <p style={{ margin: 0 }}>
            Import <strong>{folderImportDialog?.path.split(/[\\/]/).filter(Boolean).pop() ?? 'folder'}</strong>.
          </p>
          <label style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <input
              type="checkbox"
              checked={folderImportDialog?.preserveStructure ?? true}
              onChange={(event) => {
                const checked = event.currentTarget.checked;
                setFolderImportDialog((current) => (
                  current ? { ...current, preserveStructure: checked } : current
                ));
              }}
            />
            Keep the full folder structure and create it in Picto
          </label>
        </div>
      </ConfirmModal>
    </div>
  );
}
