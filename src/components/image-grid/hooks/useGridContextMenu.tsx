import { useCallback } from 'react';
import { prefetchMetadata } from '../metadataPrefetch';
import { useContextMenu } from '../../../shared/components/ContextMenu';
import type { MasonryImageItem } from '../shared';
import type { SmartFolderPredicate } from '../../smart-folders/types';
import type { GridRuntimeAction, GridRuntimeState, GridViewMode } from '../runtime';
import type { LayoutItem } from '../VirtualGrid';
import { buildGridImageContextMenu } from '../../../shared/components/context-actions/imageActions';

interface UseGridContextMenuArgs {
  scrollRef: React.RefObject<HTMLDivElement>;
  getCanvasOffsetTop: () => number;
  canvasLayoutRef: React.MutableRefObject<LayoutItem[]>;
  imagesRef: React.MutableRefObject<MasonryImageItem[]>;
  state: GridRuntimeState;
  stateRef: React.MutableRefObject<GridRuntimeState>;
  effectiveSelectedHashes: Set<string>;
  dispatch: React.Dispatch<GridRuntimeAction>;
  viewMode: GridViewMode;
  onViewModeChange?: (mode: GridViewMode) => void;
  sortField: string;
  sortOrder: string;
  onSortFieldChange?: (field: string) => void;
  onSortOrderChange?: (order: string) => void;
  smartFolderPredicate?: SmartFolderPredicate;
  smartFolderSortField?: string;
  smartFolderSortOrder?: string;
  folderId?: number | null;
  statusFilter?: string | null;
  contextMenu: ReturnType<typeof useContextMenu>;
  activateVirtualSelectAll: () => void;
  handleDeleteSelected: () => void;
  handleRestoreSelected: () => void;
  handleRemoveFromFolder: () => void;
  handleRemoveFromCollection: () => void;
  handleInboxAction: (hash: string, status: 'active' | 'trash') => void;
  handleCopyTags: () => void;
  handlePasteTags: () => void;
  hasCopiedTags: boolean;
  collectionEntityId?: number | null;
  navigateToCollection: (collection: { id: number; name: string }) => void;
  setRenameValue: React.Dispatch<React.SetStateAction<string>>;
  setRenamingHash: React.Dispatch<React.SetStateAction<string | null>>;
  renameCancelledRef: React.MutableRefObject<boolean>;
  setBatchRenameOpen: React.Dispatch<React.SetStateAction<boolean>>;
  requestGridReload: () => void;
}

export function useGridContextMenu({
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
  requestGridReload,
}: UseGridContextMenuArgs) {
  return useCallback((e: React.MouseEvent) => {
    const contextPoint = { x: e.clientX, y: e.clientY };
    const target = e.target as HTMLElement;
    if (target.closest('[data-subfolder-grid]')) {
      e.preventDefault();
      return;
    }
    // Hit-test right-click position against layout positions (canvas — no DOM tiles)
    let rightClickedHash: string | null = null;
    const container = scrollRef.current;
    if (container) {
      const containerRect = container.getBoundingClientRect();
      const canvasOffsetTop = getCanvasOffsetTop();
      const mx = e.clientX - containerRect.left + container.scrollLeft;
      const my = e.clientY - containerRect.top + container.scrollTop - canvasOffsetTop;
      const positions = canvasLayoutRef.current;
      const imgs = imagesRef.current;
      for (let i = 0; i < positions.length && i < imgs.length; i++) {
        const pos = positions[i];
        if (mx >= pos.x && mx < pos.x + pos.w && my >= pos.y && my < pos.y + pos.h) {
          rightClickedHash = imgs[i].hash;
          break;
        }
      }
      if (rightClickedHash && !effectiveSelectedHashes.has(rightClickedHash)) {
        dispatch({ type: 'SELECT_HASHES', hashes: new Set([rightClickedHash]) });
        dispatch({ type: 'DEACTIVATE_VIRTUAL_SELECT_ALL' });
        dispatch({ type: 'SET_LAST_CLICKED', hash: rightClickedHash });
        prefetchMetadata(rightClickedHash);
      }
    }

    const isMac = navigator.platform.includes('Mac');
    // After right-click hit-test, compute selection state accounting for the
    // just-applied selection (state updates are async, so selectedHashes is stale).
    const wasAlreadySelected = !!(rightClickedHash && effectiveSelectedHashes.has(rightClickedHash));
    const effectiveSize = rightClickedHash && !wasAlreadySelected ? 1 : state.selectedHashes.size;
    const effectiveVirtual = rightClickedHash && !wasAlreadySelected ? null : state.virtualAllSelection;
    const hasSingleSelection = !effectiveVirtual && effectiveSize === 1;
    const hasSelection = !!effectiveVirtual || effectiveSize > 0 || !!rightClickedHash;
    const singleHash = hasSingleSelection
      ? (rightClickedHash && !wasAlreadySelected ? rightClickedHash : [...state.selectedHashes][0])
      : rightClickedHash;
    const singleImage = singleHash ? (state.images.find((img) => img.hash === singleHash) ?? null) : null;
    const singleIsCollection = singleImage?.is_collection === true;
    const singleCollectionId = singleImage?.entity_id ?? null;

    const items = buildGridImageContextMenu({
      contextPoint,
      isMac,
      state,
      stateRef,
      imagesRef,
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
      effectiveSelectedHashes,
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
      requestGridReload,
      rightClickedHash,
      wasAlreadySelected,
      hasSelection,
      singleHash,
      singleImage,
      singleIsCollection,
      singleCollectionId,
      effectiveVirtual,
      effectiveSize,
    });
    contextMenu.open(e, items);
  }, [
    viewMode,
    onViewModeChange,
    sortField,
    sortOrder,
    onSortFieldChange,
    onSortOrderChange,
    smartFolderPredicate,
    smartFolderSortField,
    smartFolderSortOrder,
    state.selectedHashes,
    state.virtualAllSelection,
    state.virtualAllSelectedCount,
    effectiveSelectedHashes,
    handleDeleteSelected,
    handleRestoreSelected,
    handleRemoveFromFolder,
    handleInboxAction,
    statusFilter,
    state.images,
    contextMenu,
    activateVirtualSelectAll,
    handleCopyTags,
    handlePasteTags,
    folderId,
    getCanvasOffsetTop,
    dispatch,
    navigateToCollection,
    requestGridReload,
    hasCopiedTags,
    scrollRef,
    canvasLayoutRef,
    imagesRef,
    stateRef,
    setRenameValue,
    setRenamingHash,
    renameCancelledRef,
  ]);
}
