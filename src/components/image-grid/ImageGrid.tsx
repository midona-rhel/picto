import { useEffect, useLayoutEffect, useCallback, useRef, useMemo, useState } from 'react';
import { useGridRuntime } from './runtime';
import {
  effectiveSelectedHashes as selectEffectiveHashes,
  transitionOpacity,
  transitionCss,
} from './runtime';
import {
  type GridViewMode,
  type GridEmptyContext,
} from './runtime';
import { TextButton } from '../ui/TextButton';
import { StateBlock, StateActions } from '../ui/state';
import { notifySuccess, notifyError } from '../../lib/notify';
import { registerUndoAction } from '../../controllers/undoRedoController';
import { api } from '#desktop/api';
import { listen } from '#desktop/api';
import { open } from '#desktop/api';
import { getCurrentWebview } from '#desktop/api';
import { ContextMenu, useContextMenu } from '../ui/ContextMenu';
import { FileController } from '../../controllers/fileController';
import { FolderController } from '../../controllers/folderController';
import { GridController } from '../../controllers/gridController';
import { SubscriptionController } from '../../controllers/subscriptionController';
import { imageDrag } from '../../lib/imageDrag';
import { mediaThumbnailUrl } from '../../lib/mediaUrl';
import { ImageItem, MasonryImageItem, toMasonryItem } from './shared';
import { batchPreloadMediaUrls, decodeImageUrl } from './enhancedMediaCache';
import {
  prefetchMetadata,
  type SelectionQuerySpec,
} from './metadataPrefetch';
import { DetailView, type DetailViewState, type DetailViewControls } from './DetailView';
import { QuickLook } from './QuickLook';
import { computeTextHeight, TEXT_NAME_ROW_H } from './VirtualGrid';
import { CanvasGrid } from './CanvasGrid';
import type { SmartFolderPredicate } from '../smart-folders/types';
import { useGridQueryBroker, type GridQueryBrokerProps } from './queryBroker';
import { useCacheStore } from '../../stores/cacheStore';
import { useSettingsStore } from '../../stores/settingsStore';
import { useScopedDisplay } from '../../contexts/ScopedDisplayContext';
import { Slideshow } from '../Slideshow';
import { BatchRenameDialog } from '../dialogs/BatchRenameDialog';
import { useDomainStore } from '../../stores/domainStore';
import { useNavigationStore } from '../../stores/navigationStore';
import { SubfolderGrid } from './SubfolderGrid';
import { useGridMutationActions } from './hooks/useGridMutationActions';
import { useGridScopeTransition } from './hooks/useGridScopeTransition';
import { useGridHotkeys } from './hooks/useGridHotkeys';
import { useGridItemActions } from './hooks/useGridItemActions';
import { useGridKeyboardNavigation } from './hooks/useGridKeyboardNavigation';
import { useGridContextMenu } from './hooks/useGridContextMenu';
import { useGridSelection } from './hooks/useGridSelection';
import { useGridMarqueeSelection } from './hooks/useGridMarqueeSelection';
import { sortLiveImages } from './liveSort';

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
  if (statusFilter === 'untagged') return 'untagged';
  return 'default';
}

const INITIAL_PREWARM_THUMB_COUNT = 32;
const INITIAL_PREWARM_MAX_MS = 180;

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
  onSelectedImagesChange?: (images: MasonryImageItem[]) => void;
  onSelectionSummarySpecChange?: (spec: SelectionQuerySpec | null) => void;
  selectedScopeCount?: number | null;
  onDetailViewStateChange?: (state: DetailViewState | null, controls: DetailViewControls | null) => void;
  // Filter bar props
  ratingMin?: number | null;
  mimePrefixes?: string[] | null;
  colorHex?: string | null;
  colorAccuracy?: number | null;
  searchText?: string;
  externalFreeze?: boolean;
  /** Fires when scope transition fade-out completes (grid is at opacity 0). */
  onScopeTransitionMidpoint?: () => void;
}

async function prewarmInitialThumbs(items: MasonryImageItem[]): Promise<void> {
  if (items.length === 0) return;
  const urls = items
    .slice(0, INITIAL_PREWARM_THUMB_COUNT)
    .map((item) => mediaThumbnailUrl(item.hash));
  if (urls.length === 0) return;

  await Promise.race([
    Promise.allSettled(urls.map((url) => decodeImageUrl(url))),
    new Promise<void>((resolve) => window.setTimeout(resolve, INITIAL_PREWARM_MAX_MS)),
  ]);
}

export function ImageGrid({ searchTags, excludedSearchTags, tagMatchMode, smartFolderPredicate, smartFolderSortField, smartFolderSortOrder, folderId, collectionEntityId, filterFolderIds, excludedFilterFolderIds, folderMatchMode, statusFilter, viewMode = 'waterfall', targetSize = 250, onViewModeChange, sortField = 'imported_at', sortOrder = 'asc', onSortFieldChange, onSortOrderChange, onContainerWidthChange, refreshTrigger, onSelectedImagesChange, onSelectionSummarySpecChange, selectedScopeCount = null, onDetailViewStateChange, ratingMin, mimePrefixes, colorHex, colorAccuracy, searchText, externalFreeze = false, onScopeTransitionMidpoint }: ImageGridProps) {
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
  const [estimateSampleImages, setEstimateSampleImages] = useState<MasonryImageItem[]>([]);

  const displayViewModeRef = useRef(state.displayViewMode);
  displayViewModeRef.current = state.displayViewMode;
  const brokerProps: GridQueryBrokerProps = useMemo(() => ({
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
  }), [folderId, collectionEntityId, filterFolderIds, excludedFilterFolderIds, folderMatchMode, statusFilter, searchTags, excludedSearchTags, tagMatchMode, smartFolderPredicate, smartFolderSortField, smartFolderSortOrder, sortField, sortOrder, ratingMin, mimePrefixes, colorHex, colorAccuracy, searchText]);
  const { broker, queryKey, requestReplace, requestAppend } = useGridQueryBroker(
    brokerProps,
    dispatch,
    stateRef,
    displayViewModeRef,
    prewarmInitialThumbs,
    () => { initialLoadDone.current = true; },
    setEstimateSampleImages,
  );
  const queryKeyRef = useRef(queryKey);
  queryKeyRef.current = queryKey;

  const gap = 8;

  // Refs for values that change frequently but shouldn't invalidate handleImageClick
  const imagesRef = useRef(state.images);
  imagesRef.current = state.images;
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
    handleRemoveFromFolder,
    handleRemoveFromCollection,
  } = useGridMutationActions({
    stateRef,
    dispatch,
    statusFilter,
    folderId,
    collectionEntityId,
    broker,
    queryKeyRef,
  });

  // Helper: get the single selected hash (for actions that require exactly one)
  const singleSelectedHash = !state.virtualAllSelection && state.selectedHashes.size === 1
    ? [...state.selectedHashes][0]
    : null;

  const [slideshowOpen, setSlideshowOpen] = useState(false);
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
    FileController.setFileName(hash, after)
      .then(() => {
        registerUndoAction({
          label: 'Rename file',
          undo: () => api.file.setName(hash, before),
          redo: () => api.file.setName(hash, after),
        });
      })
      .catch(err => notifyError(err, 'Rename Failed'));
  }, [renameValue]);

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
    dispatch,
    navigateToCollection,
    onDetailViewStateChange,
    selectedScopeCount,
  });

  const recordImageView = useCallback((hash: string) => {
    const image = stateRef.current.images.find((img) => img.hash === hash);
    if (!image || image.is_collection) return;
    void api.file
      .incrementViewCount(hash)
      
      .catch((err) => {
        console.warn('Failed to increment view count:', err);
      });
  }, [stateRef]);

  // QuickLook intentionally skips onImageChange on mount; record the initial open here.
  useEffect(() => {
    if (!state.quickLookHash) return;
    recordImageView(state.quickLookHash);
  }, [state.quickLookHash, recordImageView]);

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
    onDetailViewStateChange,
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
    setSlideshowOpen,
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
    statusFilter,
    handleInboxAction,
  });

  const handleImageClick = useCallback((image: MasonryImageItem, event: React.MouseEvent) => {
    if (event.detail === 2) {
      dispatch({ type: 'OPEN_DETAIL', hash: image.hash });
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
  }, [dispatch, navigateToCollection]);

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
      .filter(Boolean) as MasonryImageItem[];

    const movedBefore = prev.slice(0, targetIndex).filter(img => movedSet.has(img.hash)).length;
    const insertAt = Math.max(0, Math.min(remaining.length, targetIndex - movedBefore));

    const next = [...remaining];
    next.splice(insertAt, 0, ...movedItems);

    if (currentCollectionId != null) {
      dispatch({ type: 'SET_IMAGES', images: next });
      api.collections.reorderMembers(currentCollectionId, next.map((img) => img.hash)).catch(err => {
        console.error('Collection reorder failed, reloading collection:', err);
        broker.requestReplace(queryKeyRef.current);
      });
      return;
    }

    const moves: { hash: string; after_hash?: string | null; before_hash?: string | null }[] = [];
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
      FolderController.reorderFolderItems(currentFolderId!, moves).catch(err => {
        console.error('Reorder failed, reloading folder:', err);
        broker.requestReplace(queryKeyRef.current);
      });
    }
  }, [folderId, collectionEntityId, dispatch, broker]);

  const handleImport = async () => {
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
      const result = await api.import.files(paths);
      notifySuccess(`Imported ${result.imported.length} file(s), ${result.skipped.length} skipped.`, 'Import Complete');
      requestReplace();
    } catch (err) {
      notifyError(err, 'Import Failed');
    }
  };


  const loadingMore = useRef(false);
  const loadMore = useCallback(async () => {
    if (loadingMore.current || !stateRef.current.hasMore) return;
    loadingMore.current = true;
    try {
      await requestAppend();
    } finally {
      loadingMore.current = false;
    }
  }, [requestAppend]);

  const folderIdRef = useRef(folderId);
  folderIdRef.current = folderId;

  // Reset scroll to top at the midpoint of scope transitions (after fade-out,
  // before new data renders). This prevents stale scroll positions carrying over.
  const handleScopeTransitionMidpoint = useCallback(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = 0;
    }
    onScopeTransitionMidpoint?.();
  }, [onScopeTransitionMidpoint, scrollRef]);

  const { gridFreezeActive, handleGridTransitionEnd } = useGridScopeTransition({
    state,
    dispatch,
    broker,
    queryKeyRef,
    externalFreeze,
    viewMode,
    targetSize,
    folderId: folderId ?? null,
    collectionEntityId: collectionEntityId ?? null,
    filterFolderIds,
    excludedFilterFolderIds,
    folderMatchMode: folderMatchMode ?? null,
    statusFilter,
    searchTags,
    excludedSearchTags,
    tagMatchMode: tagMatchMode ?? null,
    smartFolderPredicate,
    onDetailViewStateChange,
    onScopeTransitionMidpoint: handleScopeTransitionMidpoint,
    resolveEmptyContext: resolveGridEmptyContext,
  });

  // Sort change — reload without clearing selection (same images, different order)
  const prevSortField = useRef(sortField);
  const prevSortOrder = useRef(sortOrder);
  useEffect(() => {
    if (prevSortField.current === sortField && prevSortOrder.current === sortOrder) return;
    prevSortField.current = sortField;
    prevSortOrder.current = sortOrder;
    requestReplace();
  }, [sortField, sortOrder, requestReplace]);

  // Filter change (rating, mime, color, search text) — reload without clearing selection
  const prevRatingMin = useRef(ratingMin);
  const prevMimePrefixes = useRef(mimePrefixes);
  const prevColorHex = useRef(colorHex);
  const prevColorAccuracy = useRef(colorAccuracy);
  const prevSearchText = useRef(searchText);
  useEffect(() => {
    const mimeChanged = JSON.stringify(prevMimePrefixes.current) !== JSON.stringify(mimePrefixes);
    if (
      prevRatingMin.current === ratingMin &&
      !mimeChanged &&
      prevColorHex.current === colorHex &&
      prevColorAccuracy.current === colorAccuracy &&
      prevSearchText.current === searchText
    ) return;
    prevRatingMin.current = ratingMin;
    prevMimePrefixes.current = mimePrefixes;
    prevColorHex.current = colorHex;
    prevColorAccuracy.current = colorAccuracy;
    prevSearchText.current = searchText;
    requestReplace();
  }, [ratingMin, mimePrefixes, colorHex, colorAccuracy, searchText, requestReplace]);

  // Background refresh from subscriptions
  const prevRefreshTrigger = useRef(refreshTrigger);
  useEffect(() => {
    if (prevRefreshTrigger.current !== refreshTrigger) {
      prevRefreshTrigger.current = refreshTrigger;
      requestReplace();
    }
  }, [refreshTrigger, requestReplace]);

  // Optimistic grid removal — inspector enqueues hashes when removing from active folder.
  // Also handles detail view: images array shrinks → DetailView auto-advances.
  const pendingGridRemovals = useCacheStore((s) => s.pendingGridRemovals);
  useEffect(() => {
    if (pendingGridRemovals.size === 0) return;
    const toRemove = new Set(pendingGridRemovals);
    useCacheStore.getState().clearGridRemovals();
    dispatch({ type: 'FILTER_IMAGES', predicate: img => !toRemove.has(img.hash) });
    dispatch({ type: 'REMOVE_HASHES', hashes: toRemove });
  }, [pendingGridRemovals, dispatch]);

  // Set active grid scope for scope-aware invalidation filtering in eventBridge
  useEffect(() => {
    let scope: string;
    if (collectionEntityId != null) scope = `collection:${collectionEntityId}`;
    else if (folderId != null) scope = `folder:${folderId}`;
    else if (statusFilter === 'inbox') scope = 'system:inbox';
    else if (statusFilter === 'trash') scope = 'system:trash';
    else scope = 'system:all';
    useCacheStore.getState().setActiveGridScope(scope);
  }, [folderId, collectionEntityId, statusFilter]);

  // Patch grid tiles in-place when metadata changes (name, rating, etc.) without full reload
  const metadataInvalidatedHashes = useCacheStore((s) => s.metadataInvalidatedHashes);
  useEffect(() => {
    if (metadataInvalidatedHashes.size === 0) return;
    const hashes = [...metadataInvalidatedHashes];
    useCacheStore.getState().clearInvalidatedHashes();

    useCacheStore.getState().fetchMetadataBatch(hashes).then((results) => {
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

  // Reload grid when eventBridge bumps gridRefreshSeq (state-changed with grid_scopes)
  const gridRefreshSeq = useCacheStore((s) => s.gridRefreshSeq);
  const prevGridRefreshSeq = useRef(gridRefreshSeq);
  useEffect(() => {
    if (prevGridRefreshSeq.current !== gridRefreshSeq) {
      prevGridRefreshSeq.current = gridRefreshSeq;
      requestReplace();
    }
  }, [gridRefreshSeq, requestReplace]);

  // Real-time: listen for per-file imports and prepend/append to grid
  const searchTagsRef = useRef(searchTags);
  searchTagsRef.current = searchTags;
  const excludedSearchTagsRef = useRef(excludedSearchTags);
  excludedSearchTagsRef.current = excludedSearchTags;
  const smartFolderPredicateRef = useRef(smartFolderPredicate);
  smartFolderPredicateRef.current = smartFolderPredicate;
  const sortFieldRef = useRef(sortField);
  sortFieldRef.current = sortField;
  const sortOrderRef = useRef(sortOrder);
  sortOrderRef.current = sortOrder;
  const statusFilterRef = useRef(statusFilter);
  statusFilterRef.current = statusFilter;
  const responseTotalCountRef = useRef(state.responseTotalCount);
  responseTotalCountRef.current = state.responseTotalCount;

  useEffect(() => {
    const unlisten = listen<ImageItem>('file-imported', (event) => {
      // Skip if viewing a search, smart folder, or specific folder
      const hasTags = (searchTagsRef.current && searchTagsRef.current.length > 0)
        || (excludedSearchTagsRef.current && excludedSearchTagsRef.current.length > 0);
      const hasSmartFolder = smartFolderPredicateRef.current && smartFolderPredicateRef.current.groups.length > 0;
      if (hasTags || hasSmartFolder) return;
      if (folderIdRef.current != null) return;

      // New imports go to inbox — only live-insert when viewing inbox
      const filter = statusFilterRef.current;
      if (filter !== 'inbox') return;

      const newItem = toMasonryItem(event.payload);
      const currentImages = stateRef.current.images;
      if (currentImages.some(i => i.hash === newItem.hash)) return;
      const next = sortLiveImages(
        [newItem, ...currentImages],
        sortFieldRef.current,
        (sortOrderRef.current === 'desc' ? 'desc' : 'asc'),
      );
      dispatch({ type: 'SET_IMAGES', images: next });
      if (typeof responseTotalCountRef.current === 'number') {
        dispatch({
          type: 'SET_RESPONSE_TOTAL_COUNT',
          count: responseTotalCountRef.current + 1,
        });
      }

      batchPreloadMediaUrls([newItem], 'thumb512', 'high');
    });
    return () => { unlisten.then(fn => fn()); };
  }, [dispatch]);

  // Fallback for missed file-imported events: while subscriptions are running
  // and Inbox is active, merge newly fetched inbox items into the current grid
  // without replacing the whole dataset.
  useEffect(() => {
    const hasTags = (searchTags && searchTags.length > 0)
      || (excludedSearchTags && excludedSearchTags.length > 0);
    const hasSmartFolder = !!(smartFolderPredicate && smartFolderPredicate.groups.length > 0);
    const inInboxScope = statusFilter === 'inbox'
      && !hasTags
      && !hasSmartFolder
      && folderId == null
      && collectionEntityId == null;
    if (!inInboxScope) return;

    let disposed = false;
    let inFlight = false;
    const timer = setInterval(() => {
      if (inFlight || disposed) return;
      inFlight = true;
      void (async () => {
        try {
          const running = await SubscriptionController.getRunningSubscriptions();
          if (running.length === 0 || disposed) return;

          const page = await GridController.fetchGridPage({
            limit: 60,
            cursor: null,
            sortField: 'imported_at',
            sortOrder: 'desc',
            status: 'inbox',
          });
          if (disposed) return;

          const currentImages = stateRef.current.images;
          const currentHashes = new Set(currentImages.map((img) => img.hash));
          const incoming = page.items
            .map(toMasonryItem)
            .filter((img) => !currentHashes.has(img.hash));
          if (incoming.length === 0) return;

          const merged = sortLiveImages(
            [...incoming, ...currentImages],
            sortFieldRef.current,
            (sortOrderRef.current === 'desc' ? 'desc' : 'asc'),
          );
          dispatch({ type: 'SET_IMAGES', images: merged });
          const pageTotal = page.total_count;
          if (typeof pageTotal === 'number') {
            dispatch({ type: 'SET_RESPONSE_TOTAL_COUNT', count: pageTotal });
          } else if (typeof stateRef.current.responseTotalCount === 'number') {
            dispatch({
              type: 'SET_RESPONSE_TOTAL_COUNT',
              count: stateRef.current.responseTotalCount + incoming.length,
            });
          }
        } catch {
          // Best-effort fallback; normal event path remains primary.
        } finally {
          inFlight = false;
        }
      })();
    }, 1200);

    return () => {
      disposed = true;
      clearInterval(timer);
    };
  }, [
    statusFilter,
    searchTags,
    excludedSearchTags,
    smartFolderPredicate,
    folderId,
    collectionEntityId,
    dispatch,
  ]);

  useEffect(() => {
    const webview = getCurrentWebview();
    const promise = webview.onDragDropEvent(async (event) => {
      const payload = event.payload as any;
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
          const result = await api.import.files(paths);
          // If viewing a folder, add imported files to it
          const currentFolderId = folderIdRef.current;
          if (currentFolderId != null && result.imported?.length > 0) {
            // PBI-054: Batch add instead of per-hash fan-out.
            await FolderController.addFilesToFolderBatch(
              currentFolderId,
              result.imported,
            );
          }
          notifySuccess(`Imported ${result.imported.length} file(s), ${result.skipped.length} skipped.`, 'Import Complete');
          broker.requestReplace(queryKeyRef.current);
        } catch (err) {
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
    handleCopyTags,
    handlePasteTags,
    hasCopiedTags,
    collectionEntityId,
    navigateToCollection,
    setRenameValue,
    setRenamingHash,
    renameCancelledRef,
    setBatchRenameOpen,
    requestGridReload: () => broker.requestReplace(queryKeyRef.current),
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
        onTransitionEnd={handleGridTransitionEnd}
        style={{
          flex: 1,
          // Reserve scrollbar gutter on both sides for symmetric padding.
          overflowY: 'auto',
          scrollbarGutter: 'stable both-edges',
          overflowX: 'hidden',
          userSelect: 'none',
          WebkitUserSelect: 'none',
          opacity: state.detailHash ? 0 : transitionOpacity(state.transitionStage),
          transition: transitionCss(state.transitionStage),
          position: state.detailHash ? 'absolute' : 'relative',
          pointerEvents: (state.detailHash || gridFreezeActive) ? 'none' : 'auto',
          visibility: state.detailHash ? 'hidden' : 'visible',
          contentVisibility: state.detailHash ? 'hidden' : 'visible',
          inset: state.detailHash ? 0 : undefined,
          zIndex: state.detailHash ? -1 : undefined,
          filter: displaySettings.grayscalePreview ? 'grayscale(1)' : undefined,
        } as React.CSSProperties}
      >
          <div style={{ height: 8 }} />
          {state.displayFolderId != null && displaySettings.showSubfolders && (
            <SubfolderGrid
              folderId={state.displayFolderId}
              targetSize={state.displayTargetSize}
              totalImageCount={state.images.length}
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
            images={state.images}
            targetSize={state.displayTargetSize}
            gap={gap}
            viewMode={state.displayViewMode}
            selectedHashes={effectiveSelectedHashes}
            searchTags={state.displaySearchTags}
            onImageClick={handleImageClick}
            onImport={handleImport}
            onImportFolder={undefined}
            onContainerWidthChange={handleContainerWidthChange}
            showEmptyState={initialLoadDone.current && !hasVisibleSubfolders}
            emptyContext={state.displayEmptyContext}
            onLoadMore={state.hasMore ? loadMore : undefined}
            scrollContainerRef={scrollRef}
            popHash={state.popHash}
            onPopComplete={() => dispatch({ type: 'SET_POP_HASH', hash: null })}
            frozen={!!state.detailHash || gridFreezeActive}
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
            totalCount={state.responseTotalCount ?? selectedScopeCount}
            estimateSampleImages={estimateSampleImages}
            renamingHash={renamingHash}
          />
          {/* Inline rename overlay — positioned in layout-space so it scrolls with content */}
          {renamingHash && (() => {
            const positions = canvasLayoutRef.current;
            const imgs = imagesRef.current;
            const idx = imgs.findIndex(i => i.hash === renamingHash);
            const pos = idx >= 0 ? positions[idx] : null;
            if (!pos) return null;
            const th = computeTextHeight(displaySettings.showTileName, displaySettings.showResolution);
            const imageHeight = pos.h - th;
            // Offset by the canvas grid root's top within the scroll container
            const canvasRoot = scrollRef.current?.querySelector<HTMLElement>('[data-canvas-grid-root]');
            const offsetTop = canvasRoot?.offsetTop ?? 0;
            return (
              <input
                ref={renameInputRef}
                value={renameValue}
                onChange={(e) => setRenameValue(e.target.value)}
                onKeyDown={(e) => {
                  e.stopPropagation();
                  if (e.key === 'Enter') commitRename();
                  if (e.key === 'Escape') cancelRename();
                }}
                onBlur={commitRename}
                style={{
                  position: 'absolute',
                  top: offsetTop + pos.y + imageHeight,
                  left: pos.x,
                  width: pos.w,
                  height: TEXT_NAME_ROW_H,
                  fontSize: 'var(--font-size-md)',
                  lineHeight: '1',
                  textAlign: 'center',
                  padding: '0 4px',
                  border: '1px solid var(--color-primary)',
                  borderRadius: 3,
                  background: 'var(--color-bg-primary, #1e1e1e)',
                  color: 'var(--color-text-primary)',
                  outline: 'none',
                  boxSizing: 'border-box',
                  zIndex: 10,
                  fontFamily: 'var(--font-family)',
                }}
              />
            );
          })()}
        </div>

      {/* Detail view — replaces grid */}
      {state.detailHash && state.viewerSession && (
        <DetailView
          images={state.images}
          currentIndex={state.viewerSession.currentIndex}
          onNavigate={(delta) => dispatch({ type: 'VIEWER_NAVIGATE', delta })}
          totalCount={state.responseTotalCount ?? selectedScopeCount}
          onClose={(exitHash) => {
            dispatch({ type: 'CLOSE_DETAIL' });
            dispatch({ type: 'SET_POP_HASH', hash: exitHash });
            dispatch({ type: 'SELECT_HASHES', hashes: new Set([exitHash]) });
            dispatch({ type: 'SET_LAST_CLICKED', hash: exitHash });
            onDetailViewStateChange?.(null, null);
          }}
          onStateChange={(dvState, controls) => {
            onDetailViewStateChange?.(dvState, controls);
          }}
          onImageChange={(hash) => {
            recordImageView(hash);
            dispatch({ type: 'SELECT_HASHES', hashes: new Set([hash]) });
            dispatch({ type: 'SET_LAST_CLICKED', hash });
          }}
          onLoadMore={state.hasMore ? loadMore : undefined}
          inboxMode={statusFilter === 'inbox'}
          onInboxAction={statusFilter === 'inbox' ? handleInboxAction : undefined}
        />
      )}

      {/* QuickLook overlay — above everything */}
      {state.quickLookHash && state.viewerSession && (
        <QuickLook
          images={state.images}
          currentIndex={state.viewerSession.currentIndex}
          onNavigate={(delta) => dispatch({ type: 'VIEWER_NAVIGATE', delta })}
          totalCount={state.responseTotalCount ?? selectedScopeCount}
          onClose={(exitHash) => {
            dispatch({ type: 'CLOSE_QUICK_LOOK' });
            dispatch({ type: 'SET_POP_HASH', hash: exitHash });
          }}
          onImageChange={(hash) => {
            recordImageView(hash);
            dispatch({ type: 'SELECT_HASHES', hashes: new Set([hash]) });
            dispatch({ type: 'SET_LAST_CLICKED', hash });
            const idx = imagesRef.current.findIndex(i => i.hash === hash);
            if (idx >= 0) scrollToIndex(idx);
          }}
          onLoadMore={state.hasMore ? loadMore : undefined}
        />
      )}

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

      {slideshowOpen && state.images.length > 0 && (
        <Slideshow
          images={state.images}
          startIndex={state.images.findIndex(i => i.hash === singleSelectedHash) >= 0
            ? state.images.findIndex(i => i.hash === singleSelectedHash)
            : 0}
          onClose={() => setSlideshowOpen(false)}
        />
      )}

      <BatchRenameDialog
        opened={batchRenameOpen}
        onClose={() => setBatchRenameOpen(false)}
        images={
          state.virtualAllSelection
            ? state.images.filter(i => !state.virtualAllSelection!.excludedHashes.has(i.hash))
            : state.images.filter(i => state.selectedHashes.has(i.hash))
        }
      />
    </div>
  );
}
