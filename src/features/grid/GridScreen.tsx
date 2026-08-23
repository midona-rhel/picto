/**
 * Grid screen — feature root. Reads state, delegates to CanvasGrid.
 */

import { useEffect, useRef, useState, useCallback } from 'react';
import { useAtomValue, useSetAtom, getDefaultStore } from 'jotai';
import { IconPhoto, IconUpload, IconFolderPlus } from '@tabler/icons-react';
import * as entityMutations from '../../controllers/entityMutations';
import { getShortcut, matchesShortcutDef } from '../../shared/lib/shortcuts';
import { activeNodeIdAtom } from '../../state/navigation';
import {
  gridItemsAtom,
  gridLoadingAtom,
  gridErrorAtom,
  gridCursorAtom,
  gridViewModeAtom,
  gridTargetSizeAtom,
  gridShowNameAtom,
  gridShowExtensionAtom,
  gridShowExtensionLabelAtom,
  gridShowResolutionAtom,
  gridFitThumbnailsAtom,
  gridSearchTextAtom,
  gridTotalCountAtom,
  gridTotalSizeBytesAtom,
  gridScopeAtom,
  gridShowSubfoldersAtom,
  gridChildFoldersAtom,
  type GridTransitionPhase,
} from '../../state/grid';
import { gridController } from '../../controllers/gridController';
import { foldersController } from '../../controllers/foldersController';
import { isNativeDragPending, isDragActive as isDragActiveCheck, isInternalDragOrigin } from './dragState';
import {
  clearSelectionAtom,
  gridSelectionActionAtom,
  gridSelectionAtom,
  loadedSelectedItemIdsAtom,
  selectAllResultsAtom,
  selectionCountAtom,
  selectionTargetAtom,
} from '../../state/selection';
import {
  displayedGridSnapshotAtom,
  displayedInspectorTargetAtom,
  displayedInspectorItemDetailsAtom,
  inspectorLoadingAtom,
  inspectorErrorAtom,
  liveInspectorTargetAtom,
} from '../../state/inspector';
import { sidebarNodesAtom } from '../../state/sidebar';
import { CanvasGrid } from './canvas/CanvasGrid';
import { SubfolderGrid, type SubfolderGridHandle } from './SubfolderGrid';
import { ContextMenu, useContextMenu } from '../../shared/ui/ContextMenu';
import { buildTileContextMenu, buildEmptyContextMenu } from './gridContextMenu';
import { pushHistory } from '../../state/navigationHistory';
import { viewerSessionAtom, quickLookSessionAtom, createViewerSession, navigateViewerSession, resolveViewerIndex } from '../../state/viewer';
import { aiTaggerPortalAtom, inspectorAnchor } from '../../state/portals';
import { confirmModalAtom, folderImportModalAtom, exportModalAtom, tagSelectModalAtom, folderPickerModalAtom } from '../../state/modals';
import { MediaView } from '../viewer/MediaView';
import { QuickLook } from '../viewer/QuickLook';
import { TagSelectPanel } from '../tags/TagSelectPanel';
import { FolderPickerPanel } from '../folders/FolderPickerPanel';
import { AiTaggerPanel } from '../ai-tagger/AiTaggerPanel';
import { useGridArrowNav } from './hooks/useGridArrowNav';
import type { LayoutResult } from './layout/types';
import { windowController } from '../../controllers/windowController';
import { filesController, manualImportParamsForScope } from '../../controllers/filesController';
import { viewerController } from '../../controllers/viewerController';
import { EmptyState, EmptyStateAction } from '../../shared/ui/EmptyState';
import { ApplicationMenuButton } from '../../shared/ui/ApplicationMenuButton/ApplicationMenuButton';
import { scrollGridItemIntoView, type GridScrollAlignment } from './gridScroll';
import { resolveContextMenuTarget } from './gridMenuSelection';
import styles from './GridScreen.module.css';
import type { Lifecycle } from '../../shared/types/generated/application/Lifecycle';

const store = getDefaultStore();
function supportsExplicitImageAutoTagging(
  querySelectionActive: boolean,
  itemIds: Set<number>,
  items: Array<{ item_id: number; display_mime_type: string }>,
): boolean {
  if (querySelectionActive || itemIds.size === 0) {
    return false;
  }
  const selectedItems = items.filter((item) => itemIds.has(item.item_id));
  return (
    selectedItems.length === itemIds.size &&
    selectedItems.every((item) => item.display_mime_type.startsWith('image/'))
  );
}

interface GridScreenProps {
  nodeId?: string;
  transitionPhase?: GridTransitionPhase;
  initialScrollTop?: number | null;
  onFirstPaint?: () => void;
  onScrollTopChange?: (scrollTop: number) => void;
}

export function GridScreen({
  nodeId,
  transitionPhase = 'idle',
  initialScrollTop = null,
  onFirstPaint,
  onScrollTopChange,
}: GridScreenProps) {
  const activeNodeId = useAtomValue(activeNodeIdAtom);
  const displayedNodeId = nodeId ?? activeNodeId;
  const setActiveNodeId = useSetAtom(activeNodeIdAtom);
  const items = useAtomValue(gridItemsAtom);
  const loading = useAtomValue(gridLoadingAtom);
  const error = useAtomValue(gridErrorAtom);
  const cursor = useAtomValue(gridCursorAtom);
  const viewMode = useAtomValue(gridViewModeAtom);
  const targetSize = useAtomValue(gridTargetSizeAtom);
  const showName = useAtomValue(gridShowNameAtom);
  const showExtension = useAtomValue(gridShowExtensionAtom);
  const showExtensionLabel = useAtomValue(gridShowExtensionLabelAtom);
  const showResolution = useAtomValue(gridShowResolutionAtom);
  const fitThumbnails = useAtomValue(gridFitThumbnailsAtom);
  const searchText = useAtomValue(gridSearchTextAtom);
  const totalCount = useAtomValue(gridTotalCountAtom);
  const totalSizeBytes = useAtomValue(gridTotalSizeBytesAtom);
  const gridScope = useAtomValue(gridScopeAtom);
  const sidebarNodes = useAtomValue(sidebarNodesAtom);
  const selection = useAtomValue(gridSelectionAtom);
  const selectedItemIds = useAtomValue(loadedSelectedItemIdsAtom);
  const selectedSubfolderNodeIds = selection.folderNodeIds;
  const selectionMode = selection.mode;
  const querySelectionActive = selection.mode === 'query_results';
  const selectionCount = useAtomValue(selectionCountAtom);
  const selectionTarget = useAtomValue(selectionTargetAtom);
  const dispatchSelection = useSetAtom(gridSelectionActionAtom);
  const clearSelection = useSetAtom(clearSelectionAtom);
  const selectAllResults = useSetAtom(selectAllResultsAtom);
  const viewerSession = useAtomValue(viewerSessionAtom);
  const setViewerSession = useSetAtom(viewerSessionAtom);
  const quickLookSession = useAtomValue(quickLookSessionAtom);
  const setQuickLookSession = useSetAtom(quickLookSessionAtom);
  const setAiTaggerPortal = useSetAtom(aiTaggerPortalAtom);
  const setTagSelectModal = useSetAtom(tagSelectModalAtom);
  const setFolderPickerModal = useSetAtom(folderPickerModalAtom);
  const gridContainerRef = useRef<HTMLDivElement | null>(null);
  const gridLayoutRef = useRef<LayoutResult | null>(null);
  const [renamingIndex, setRenamingIndex] = useState<number | null>(null);
  const subfolderGridRef = useRef<SubfolderGridHandle>(null);

  const scrollToItem = useCallback((index: number, alignment: GridScrollAlignment = 'nearest') => {
    const layout = gridLayoutRef.current;
    const container = gridContainerRef.current;
    if (!layout || !container || index < 0) return;
    const nextScrollTop = scrollGridItemIntoView(container, layout, index, alignment);
    if (nextScrollTop != null) lastScrollTopRef.current = nextScrollTop;
  }, []);

  useGridArrowNav({
    items,
    layoutRef: gridLayoutRef,
    containerRef: gridContainerRef,
    selectedItemIds,
    selection,
    dispatchSelection,
    viewerOpen: !!(viewerSession || quickLookSession),
    containerWidth: gridContainerRef.current?.clientWidth ?? 0,
    targetSize,
  });

  // ── External file drop (drag from OS into app) ──
  const [fileDragOver, setFileDragOver] = useState(false);
  const gridScopeRef2 = useRef(gridScope);
  gridScopeRef2.current = gridScope;

  useEffect(() => {
    const webview = (window as any).picto?.webview;
    if (!webview?.onDragDropEvent) return;

    const promise = webview.onDragDropEvent((event: { payload: { type: string; paths?: string[] } }) => {
      const { type, paths } = event.payload;
      // Completely ignore all drag events while any app-originated drag is active
      if (isNativeDragPending() || isDragActiveCheck() || isInternalDragOrigin()) return;
      if (type === 'enter') { setFileDragOver(true); return; }
      if (type === 'leave') { setFileDragOver(false); return; }
      if (type !== 'drop' || !paths?.length) return;
      setFileDragOver(false);

      const scope = gridScopeRef2.current;
      const folderId = scope.kind === 'folder' ? scope.folder_id : null;

      // Detect folder drop (single path without media extension)
      const mediaExt = /\.(jpe?g|png|gif|webp|bmp|tiff?|svg|mp4|mkv|webm|avi|mov|wmv|flv|m4v|avif|jxl|ico|pdf)$/i;
      if (paths.length === 1 && !mediaExt.test(paths[0])) {
        // Show import modal for folder drops
        store.set(folderImportModalAtom, {
          open: true,
          path: paths[0],
          targetFolderId: folderId ?? null,
          lifecycle: manualImportParamsForScope(scope).lifecycle,
        });
      } else {
        // File import — direct
        void filesController.addMedia(paths, manualImportParamsForScope(scope,
          folderId != null ? { parent_folder_id: folderId } : {}));
      }
    });

    return () => { promise.then((fn: () => void) => fn()); };
  }, []);

  const showSubfolders = useAtomValue(gridShowSubfoldersAtom);
  const childFolders = useAtomValue(gridChildFoldersAtom);
  const contextMenu = useContextMenu();

  const setDisplayedGridSnapshot = useSetAtom(displayedGridSnapshotAtom);
  const setDisplayedInspectorTarget = useSetAtom(displayedInspectorTargetAtom);
  const setDisplayedEntityData = useSetAtom(displayedInspectorItemDetailsAtom);
  const setInspectorLoading = useSetAtom(inspectorLoadingAtom);
  const setInspectorError = useSetAtom(inspectorErrorAtom);
  const liveTarget = useAtomValue(liveInspectorTargetAtom);

  const lastScrollTopRef = useRef(0);

  // Commit the displayed scene — snapshot + inspector target — atomically.
  // ONLY commits during fading_in (new data arriving after transition).
  // During idle: only commits if data changed within the SAME scope (reconcile, sort, search).
  const displayedNodeIdRef = useRef(displayedNodeId);

  useEffect(() => {
    // During fading_in: commit only when data is loaded (not loading)
    // During idle: only commit if we're on the SAME scope (data update, not scope change)
    const isSameScope = displayedNodeId === displayedNodeIdRef.current;
    const shouldCommit = (transitionPhase === 'fading_in' && !loading) || (transitionPhase === 'idle' && isSameScope);

    if (shouldCommit) {
      displayedNodeIdRef.current = displayedNodeId;
      setDisplayedGridSnapshot({
        nodeId: displayedNodeId,
        previewItems: items.slice(0, 4),
        totalCount,
        totalSizeBytes,
        searchText: searchText.trim(),
        sidebarNode: sidebarNodes.find((n) => n.id === displayedNodeId) ?? null,
      });
      // Don't overwrite inspector target when a subfolder tile is selected
      // (liveTarget points to the subfolder, not the current scope)
      const isSubfolderSelected = liveTarget.kind === 'scope' && 'nodeId' in liveTarget && liveTarget.nodeId !== displayedNodeId;
      if (!isSubfolderSelected && (transitionPhase === 'fading_in' || liveTarget.kind === 'scope' || liveTarget.kind === 'none')) {
        setDisplayedInspectorTarget(
          liveTarget.kind === 'none'
            ? { kind: 'none' }
            : { kind: 'scope', nodeId: displayedNodeId },
        );
        setDisplayedEntityData(null);
        setInspectorLoading(false);
        setInspectorError(null);
      }
    }
  }, [
    displayedNodeId,
    items,
    liveTarget,
    searchText,
    setDisplayedGridSnapshot,
    setDisplayedInspectorTarget,
    setDisplayedEntityData,
    setInspectorLoading,
    setInspectorError,
    totalCount,
    totalSizeBytes,
    transitionPhase,
  ]); // eslint-disable-line react-hooks/exhaustive-deps -- sidebarNodes intentionally excluded: snapshot freezes node at commit time



  const addSelectionToFolder = useCallback(() => {
    if (!selectionTarget) return;
    setFolderPickerModal({ open: true });
  }, [selectionTarget, setFolderPickerModal]);

  const removeSelectionFromCurrentFolder = useCallback(async (target = selectionTarget) => {
    if (!target || gridScope.kind !== 'folder') return;
    await entityMutations.updateTargetFolderMembership(target, gridScope.folder_id, 'remove');
  }, [gridScope, selectionTarget]);

  const setSelectionLifecycle = useCallback(async (lifecycle: Lifecycle, target = selectionTarget) => {
    if (!target) return;
    await entityMutations.setTargetLifecycle(target, lifecycle);
  }, [selectionTarget]);

  const permanentlyDeleteSelection = useCallback((target = selectionTarget, count = selectionCount) => {
    if (!target) return;
    store.set(confirmModalAtom, {
      open: true,
      title: 'Delete Permanently',
      message: `This will permanently delete ${count} item${count !== 1 ? 's' : ''}. This cannot be undone.`,
      confirmLabel: 'Delete',
      danger: true,
      onConfirm: () => {
        void entityMutations.permanentlyDeleteTarget(target);
        clearSelection();
      },
    });
  }, [selectionTarget, selectionCount, clearSelection]);

  // ── Detail window communication ──
  // When a detail window opens, it sends 'detail-window-ready' with { hash }.
  // We respond with ONLY the selected images (not the entire grid).
  // Single selection → one image, no navigation in detail window.
  // Multi selection → those images as a navigable set.
  const detailWindowSelectionRef = useRef(new Map<string, number[]>());
  useEffect(() => {
    const picto = (window as any).picto;
    if (!picto?.events?.on) return;
    let cancelled = false;
    const p = picto.events.on('detail-window-ready', (payload: any) => {
      if (cancelled) return;
      const readyHash = payload?.hash;
      if (!readyHash) return;
      const label = `detail-${readyHash.slice(0, 12)}`;
      // Look up what we stored when Cmd+O was pressed
      const selectedItemIds = detailWindowSelectionRef.current.get(label);
      if (!selectedItemIds) return;
      const curItems = itemsRef.current;
      const lightImages = selectedItemIds
        .map((itemId: number) => curItems.find((i: any) => i.item_id === itemId))
        .filter(Boolean)
        .map((i: any) => ({
          hash: i.display_file_hash,
          name: i.name,
          mime: i.display_mime_type,
          width: i.pixel_width,
          height: i.pixel_height,
        }));
      picto.events.emitTo(label, 'detail-images', {
        images: lightImages,
        totalCount: lightImages.length,
      });
    });
    return () => { cancelled = true; p.then((fn: any) => fn?.()).catch(() => {}); };
  }, []);

  // ── Grid keyboard shortcuts ──
  // Refs for values that change frequently — avoids re-registering the listener.
  const selectionCountRef = useRef(selectionCount);
  selectionCountRef.current = selectionCount;
  const selectedItemIdsRef = useRef(selectedItemIds);
  selectedItemIdsRef.current = selectedItemIds;
  const itemsRef = useRef(items);
  itemsRef.current = items;
  const querySelectionActiveRef = useRef(querySelectionActive);
  querySelectionActiveRef.current = querySelectionActive;
  const viewerSessionRef = useRef(viewerSession);
  viewerSessionRef.current = viewerSession;
  const quickLookSessionRef = useRef(quickLookSession);
  quickLookSessionRef.current = quickLookSession;
  const gridScopeRef = useRef(gridScope);
  gridScopeRef.current = gridScope;

  // Refs for setters used in the keydown handler (avoid re-registering on every render)
  const setTagSelectModalRef = useRef(setTagSelectModal);
  setTagSelectModalRef.current = setTagSelectModal;
  const setFolderPickerModalRef = useRef(setFolderPickerModal);
  setFolderPickerModalRef.current = setFolderPickerModal;
  const setAiTaggerPortalRef = useRef(setAiTaggerPortal);
  setAiTaggerPortalRef.current = setAiTaggerPortal;

  useEffect(() => {
    const defs = {
      selectAll:       getShortcut('edit.selectAll')!,
      deselectAll:     getShortcut('edit.deselectAll')!,
      detailView:      getShortcut('view.detailView')!,
      quicklook:       getShortcut('view.quicklook')!,
      delete_:         getShortcut('file.delete')!,
      restore:         getShortcut('file.restore')!,
      addToFolder:     getShortcut('file.addToFolder')!,
      removeFromFolder: getShortcut('file.removeFromFolder')!,
      openDefault:     getShortcut('file.openDefaultApp')!,
      revealInFolder:  getShortcut('file.revealInFolder')!,
      openNewWindow:   getShortcut('file.openNewWindow')!,
      addTag:          getShortcut('organize.addTag')!,
      addToFolders:    getShortcut('organize.addFolder')!,
      autoTag:         getShortcut('organize.autoTag')!,
    };

    function handleKeyDown(e: KeyboardEvent) {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      // Don't handle grid shortcuts while a viewer is open — the viewer handles its own keys.
      // Exception: detailView/quicklook shortcuts to open viewers are checked below with their own guards.
      if (viewerSessionRef.current || quickLookSessionRef.current) {
        // Only allow viewer-opening shortcuts through (they have their own viewer-state guards)
        if (!matchesShortcutDef(e, defs.detailView) && !matchesShortcutDef(e, defs.quicklook)) return;
      }
      const count = selectionCountRef.current;
      const itemIds = selectedItemIdsRef.current;
      const curItems = itemsRef.current;
      const scope = gridScopeRef.current;
      const isTrash = scope.kind === 'trash';
      const singleItemId = count === 1 ? [...itemIds][0] : null;
      const singleItem = singleItemId == null
        ? null
        : curItems.find((item) => item.item_id === singleItemId) ?? null;
      const singleFileHash = singleItem?.display_file_hash ?? null;
      const canAutoTag = supportsExplicitImageAutoTagging(
        querySelectionActiveRef.current,
        itemIds,
        curItems,
      );

      if (matchesShortcutDef(e, defs.selectAll)) { e.preventDefault(); selectAllResults(); return; }
      if (matchesShortcutDef(e, defs.deselectAll) && count > 0) { clearSelection(); return; }

      if (matchesShortcutDef(e, defs.detailView) && singleItemId != null && !viewerSessionRef.current && !quickLookSessionRef.current) {
        e.preventDefault(); setViewerSession(createViewerSession(curItems, singleItemId)); return;
      }
      if (matchesShortcutDef(e, defs.quicklook) && !viewerSessionRef.current) {
        e.preventDefault();
        if (quickLookSessionRef.current) setQuickLookSession(null);
        else if (singleItemId != null) setQuickLookSession(createViewerSession(curItems, singleItemId));
        return;
      }

      if (matchesShortcutDef(e, defs.openDefault) && singleFileHash) {
        e.preventDefault(); void filesController.openDefaultAppForHash(singleFileHash); return;
      }
      if (matchesShortcutDef(e, defs.revealInFolder) && singleFileHash) {
        e.preventDefault(); void filesController.revealHashInFolder(singleFileHash); return;
      }
      if (matchesShortcutDef(e, defs.openNewWindow) && count > 0) {
        e.preventDefault();
        // Use first selected hash as the window identity
        const selectedArr = [...itemIds];
        const item = singleItem ?? curItems.find((candidate) => candidate.item_id === selectedArr[0]);
        if (!item) return;
        const primaryHash = item.display_file_hash;
        const label = `detail-${primaryHash.slice(0, 12)}`;
        detailWindowSelectionRef.current.set(label, selectedArr);
        void windowController.openDetailWindow({
          hash: primaryHash,
          width: item?.pixel_width ?? null,
          height: item?.pixel_height ?? null,
        });
        return;
      }

      // Mod+Backspace: context-dependent destructive action
      if (matchesShortcutDef(e, defs.delete_) && count > 0) {
        e.preventDefault();
        if (isTrash) void permanentlyDeleteSelection();
        else void setSelectionLifecycle('trash');
        return;
      }
      // Mod+Shift+Backspace: context-dependent reverse action
      if (matchesShortcutDef(e, defs.restore) && count > 0) {
        e.preventDefault();
        if (isTrash) void setSelectionLifecycle('active');
        else if (scope.kind === 'folder') void removeSelectionFromCurrentFolder();
        return;
      }

      if (matchesShortcutDef(e, defs.addToFolder) && count > 0) {
        e.preventDefault(); void addSelectionToFolder(); return;
      }

      // T — open tag select modal, F — open folder picker modal
      if (matchesShortcutDef(e, defs.addTag) && count > 0) {
        e.preventDefault(); setTagSelectModalRef.current({ open: true }); return;
      }
      if (matchesShortcutDef(e, defs.addToFolders) && count > 0) {
        e.preventDefault(); setFolderPickerModalRef.current({ open: true }); return;
      }
      if (matchesShortcutDef(e, defs.autoTag) && canAutoTag) {
        e.preventDefault(); setAiTaggerPortalRef.current({ open: true, anchor: inspectorAnchor() }); return;
      }

      // Rating keys 0-5 (plain digits, no modifiers)
      if (!e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey && count > 0) {
        const digit = parseInt(e.key, 10);
        if (digit >= 0 && digit <= 5) {
          e.preventDefault();
          void entityMutations.setTargetRating(
            { kind: 'explicit', item_ids: [...itemIds] },
            digit,
          );
          return;
        }
      }
    }
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [clearSelection, selectAllResults, setViewerSession, setQuickLookSession, setSelectionLifecycle, permanentlyDeleteSelection, addSelectionToFolder, removeSelectionFromCurrentFolder]);

  const isEmpty = items.length === 0 && !loading;

  const renderIncomingSurface = () => {
    if (error) {
      return (
        <div className={styles.error}>
          <span>{error}</span>
          <button className={styles.retryBtn} onClick={() => gridController.loadFirstPage()}>
            Retry
          </button>
        </div>
      );
    }

    if (isEmpty) {
      const scopeKey = gridScope.kind;
      const hasSearch = searchText.trim().length > 0;
      const emptyTitle = hasSearch ? 'No results found'
        : scopeKey === 'inbox' ? 'Inbox is empty'
        : scopeKey === 'uncategorized' ? 'No uncategorized images'
        : scopeKey === 'untagged' ? 'No untagged images'
        : scopeKey === 'smart_folder' ? 'No matching images'
        : scopeKey === 'folder' ? 'This folder is empty'
        : 'No images';
      const emptyDesc = hasSearch ? 'Try different search terms or clear filters'
        : scopeKey === 'inbox' ? 'Run subscriptions to add new images to your inbox'
        : scopeKey === 'uncategorized' ? 'All your images are already assigned to folders'
        : scopeKey === 'untagged' ? 'All your images have been tagged'
        : scopeKey === 'smart_folder' ? 'Try adjusting the rules for this smart folder'
        : scopeKey === 'folder' ? 'Drag and drop files here, or import them below'
        : 'Drag and drop files here, or click the button below to import';
      const showImport = !hasSearch && scopeKey !== 'inbox' && scopeKey !== 'untagged' && scopeKey !== 'smart_folder';

      return (
        <EmptyState
          icon={<IconPhoto size={28} stroke={1.2} style={{ color: 'var(--color-bg-app)', opacity: 1 }} />}
          title={emptyTitle}
          description={emptyDesc}
          actions={showImport ? (
            <>
              <EmptyStateAction onClick={() => {
                void (async () => {
                  try {
                    const result = await (window as any).picto.dialog.open({
                      properties: ['openFile'], multiple: true, title: 'Import files',
                      filters: [{ name: 'Media', extensions: ['png','jpg','jpeg','gif','webp','bmp','mp4','webm','mkv','mov','avi'] }],
                    });
                    if (result) {
                      const paths = Array.isArray(result) ? result : [result];
                      await filesController.addMedia(paths, manualImportParamsForScope(gridScope,
                        gridScope.kind === 'folder' ? { parent_folder_id: gridScope.folder_id } : {}));
                    }
                  } catch (err) {
                    console.error('[grid] import files failed:', err);
                  }
                })();
              }}>
                <IconUpload size={14} stroke={1.5} />
                Import Files
              </EmptyStateAction>
              <EmptyStateAction onClick={() => {
                void (async () => {
                  try {
                    const result = await (window as any).picto.dialog.open({
                      properties: ['openDirectory'], multiple: false, title: 'Import folder',
                    });
                    if (result) {
                      const folderPath = typeof result === 'string' ? result : result[0];
                      await filesController.addMedia([folderPath], manualImportParamsForScope(gridScope, {
                        preserve_structure: true,
                        parent_folder_id: gridScope.kind === 'folder' ? gridScope.folder_id : null,
                      }));
                    }
                  } catch (err) {
                    console.error('[grid] import folder failed:', err);
                  }
                })();
              }}>
                <IconFolderPlus size={14} stroke={1.5} />
                Import Folder
              </EmptyStateAction>
            </>
          ) : undefined}
        />
      );
    }

    const hasSubfolders = showSubfolders && childFolders.length > 0 && gridScope.kind === 'folder';

    const subfolderHeader = hasSubfolders ? (
      <SubfolderGrid
        ref={subfolderGridRef}
        childFolders={childFolders}
        targetSize={targetSize}
        totalImageCount={items.length}
        selectedNodeIds={selectedSubfolderNodeIds}
        onOpenFolder={(nodeId) => {
          pushHistory(nodeId);
          setActiveNodeId(nodeId);
        }}
        onSelectFolder={(nodeId, event) => {
          if (event.metaKey || event.ctrlKey) {
            dispatchSelection({ type: 'toggle_folder', id: nodeId });
          } else {
            dispatchSelection({ type: 'replace_folders', ids: new Set([nodeId]), anchor: nodeId });
          }
        }}
        onFolderContextMenu={(nodeId, _folder, pos) => {
          // Select the folder if not already selected
          if (!selectedSubfolderNodeIds.has(nodeId)) {
            dispatchSelection({ type: 'replace_folders', ids: new Set([nodeId]), anchor: nodeId });
          }
          const folderId = parseInt(nodeId.replace('folder:', ''), 10);
          if (isNaN(folderId)) return;
          const entries = buildTileContextMenu({
            selectionCount: 1,
            querySelectionActive: false,
            singleSelected: true,
            singleHash: nodeId,
            hasFolders: true,
            isMixed: false,
            isFoldersOnly: true,
            scopeKind: 'folder',
            statusFilter: null,
            loadedCount: items.length,
            onSelectAll: () => selectAllResults(),
            onDeselectAll: () => clearSelection(),
            onOpen: () => {
              pushHistory(nodeId);
              setActiveNodeId(nodeId);
            },
          });
          contextMenu.openAt(pos, entries);
        }}
      />
    ) : undefined;

    return (
      <CanvasGrid
        items={items}
        headerContent={subfolderHeader}
        dragSourceScope={gridScope}
        viewMode={viewMode}
        targetSize={targetSize}
        showName={showName}
        showExtension={showExtension}
        showExtensionLabel={showExtensionLabel}
        showResolution={showResolution}
        fitThumbnails={fitThumbnails}
        interactive={!viewerSession && !quickLookSession}
        suppressTileReveal={transitionPhase === 'fading_out' || transitionPhase === 'waiting'}
        selectedItemIds={selectedItemIds}
        selectedFolderNodeIds={selectedSubfolderNodeIds}
        initialScrollTop={initialScrollTop}
        onContainerRef={(el) => { gridContainerRef.current = el; }}
        onLayoutChange={(l) => { gridLayoutRef.current = l; }}
        renamingIndex={renamingIndex}
        onRenameCommit={(idx, name) => {
          setRenamingIndex(null);
          const item = items[idx];
          if (item && name) void entityMutations.setItemName(item.item_id, name);
        }}
        onRenameCancel={() => setRenamingIndex(null)}
        onFirstPaint={onFirstPaint}
        onScrollTopChange={(scrollTop) => { lastScrollTopRef.current = scrollTop; onScrollTopChange?.(scrollTop); }}
        onTileClick={(index, item, event) => {
          const itemId = item.item_id;
          if (event?.shiftKey && selection.anchor?.kind === 'item') {
            const anchorIndex = items.findIndex((entry) => entry.item_id === selection.anchor!.id);
            const from = Math.min(anchorIndex >= 0 ? anchorIndex : index, index);
            const to = Math.max(anchorIndex >= 0 ? anchorIndex : index, index);
            const base = (event.metaKey || event.ctrlKey)
              ? new Set(selectedItemIds)
              : new Set<number>();
            for (let i = from; i <= to; i++) {
              if (items[i]) base.add(items[i].item_id);
            }
            dispatchSelection({ type: 'range_items', itemIds: base });
          } else if (event?.metaKey || event?.ctrlKey) {
            dispatchSelection(selectionMode === 'query_results'
              ? { type: 'toggle_query_item', itemId, totalCount: totalCount ?? items.length }
              : { type: 'toggle_item', itemId });
          } else {
            dispatchSelection({ type: 'replace_items', itemIds: new Set([itemId]), anchor: itemId });
          }
        }}
        onTileDoubleClick={(_index, item) => {
          setViewerSession(createViewerSession(items, item.item_id));
        }}
        onEmptyClick={() => clearSelection()}
        onSelectionChange={(itemIds) => dispatchSelection({ type: 'replace_items', itemIds })}
        onMarqueeSelectionChange={({ itemIds, folderNodeIds }) => {
          dispatchSelection({ type: 'marquee', itemIds, folderNodeIds, additive: false });
        }}
        collectHeaderMarqueeHits={(rect) => subfolderGridRef.current?.collectMarqueeHits(rect) ?? new Set()}
        onTileContextMenu={(_index, item, pos) => {
          // Ensure the right-clicked tile is selected
          let effectiveItemIds = selectedItemIds;
          let effectiveSelectionMode = selectionMode;
          let effectiveSelectionCount = selectionCount;
          let effectiveQuerySelectionActive = querySelectionActive;
          if (!selectedItemIds.has(item.item_id)) {
            effectiveItemIds = new Set([item.item_id]);
            dispatchSelection({ type: 'replace_items', itemIds: effectiveItemIds, anchor: item.item_id });
            effectiveSelectionMode = 'explicit';
            effectiveSelectionCount = 1;
            effectiveQuerySelectionActive = false;
          }

          // Derive context for menu builder
          const selCount = effectiveSelectionCount;
          const selectedItems = items.filter((it) => effectiveItemIds.has(it.item_id));
          const singleItem = effectiveSelectionMode === 'explicit' && selCount === 1 ? selectedItems[0] : null;
          const canAutoTag = effectiveSelectionMode === 'explicit'
            && selectedItems.length === effectiveItemIds.size
            && selectedItems.every((selected) => selected.display_mime_type.startsWith('image/'));
          const effectiveTarget = resolveContextMenuTarget(
            effectiveQuerySelectionActive,
            selectionTarget,
            effectiveItemIds,
          );
          const scopeKind = gridScope.kind === 'folder' ? 'folder'
            : gridScope.kind === 'smart_folder' ? 'smart_folder'
            : 'system';
          const statusFilter = gridScope.kind === 'inbox' ? 'inbox'
            : gridScope.kind === 'trash' ? 'trash'
            : gridScope.kind === 'all' ? 'active'
            : null;

          const entries = buildTileContextMenu({
            selectionCount: selCount,
            querySelectionActive: effectiveQuerySelectionActive,
            aiTagEnabled: canAutoTag,
            singleSelected: effectiveSelectionMode === 'explicit' && selCount === 1,
            singleHash: singleItem?.display_file_hash ?? null,
            scopeKind,
            statusFilter,
            loadedCount: items.length,
            onSelectAll: () => selectAllResults(),
            onDeselectAll: () => clearSelection(),
            onOpen: singleItem ? () => setViewerSession(createViewerSession(items, singleItem.item_id)) : undefined,
            onOpenNewWindow: (hash) => {
              const it = items.find((i) => i.display_file_hash === hash);
              const selectedArr = [...effectiveItemIds];
              const label = `detail-${hash.slice(0, 12)}`;
              detailWindowSelectionRef.current.set(label, selectedArr);
              void windowController.openDetailWindow({
                hash,
                width: it?.pixel_width ?? null,
                height: it?.pixel_height ?? null,
              });
            },
            onOpenDefault: (hash) => { void filesController.openDefaultAppForHash(hash); },
            onRevealInFolder: (hash) => { void filesController.revealHashInFolder(hash); },
            onCopyFilePath: (hash) => { void filesController.copyFilePath(hash); },
            onCopyFile: (hash) => { void filesController.copyFileForHash(hash); },
            onCopyName: (name) => { filesController.copyText(name); },
            singleName: singleItem?.name ?? null,
            singleMime: singleItem?.display_mime_type ?? null,
            onCopyLink: (hash, mime) => {
              const ext: Record<string, string> = { 'image/jpeg': 'jpg', 'image/png': 'png', 'image/gif': 'gif', 'image/webp': 'webp', 'video/mp4': 'mp4', 'video/webm': 'webm' };
              filesController.copyText(`media://localhost/file/${hash}.${ext[mime] ?? 'bin'}`);
            },
            onRename: singleItem ? () => {
              const idx = items.findIndex((i) => i.item_id === singleItem.item_id);
              if (idx >= 0) setRenamingIndex(idx);
            } : undefined,
            onRegenerateThumbnails: () => {
              const hashes = selectedItems.map((selected) => selected.display_file_hash);
              void filesController.regenerateThumbnailsBatch(hashes);
            },
            onCopyTags: () => {
              if (!singleItem) return;
              void viewerController.getItemDetails(singleItem.item_id).then((d) => {
                if (!d?.aggregate_tags) return;
                const tagStrings = d.aggregate_tags;
                filesController.copyText(JSON.stringify(tagStrings));
                (window as any).__pictoClipboardTags = tagStrings;
              });
            },
            onPasteTags: () => {
              const tags = (window as any).__pictoClipboardTags as string[] | undefined;
              if (!tags?.length || !effectiveTarget) return;
              void entityMutations.addTargetTags(effectiveTarget, tags);
            },
            hasClipboardTags: !!((window as any).__pictoClipboardTags as string[] | undefined)?.length,
            onAddToFolder: () => { setFolderPickerModal({ open: true }); },
            onNewFolderWithSelection: effectiveTarget ? () => {
              void (async () => {
                const name = 'New Folder';
                const nodeId = await foldersController.create(name);
                if (!nodeId) return;
                const folderId = parseInt(nodeId.replace('folder:', ''), 10);
                if (isNaN(folderId)) return;
                await entityMutations.updateTargetFolderMembership(effectiveTarget, folderId, 'add');
              })();
            } : undefined,
            onSearchByImage: (engine, hash) => {
              const urls: Record<string, string> = {
                tineye: `https://tineye.com/search/?url=`,
                saucenao: `https://saucenao.com/search.php?url=`,
                yandex: `https://yandex.com/images/search?rpt=imageview&url=`,
                bing: `https://www.bing.com/images/search?view=detailv2&iss=sbi&form=SBIVSP&sbisrc=UrlPaste&q=imgurl:`,
              };
              // Use the thumbnail URL as the search source
              const thumbUrl = `media://localhost/thumb/${hash}.jpg`;
              const url = urls[engine];
              if (url) void (window as any).picto?.shell?.openExternal(url + encodeURIComponent(thumbUrl));
            },
            onSetRating: (rating) => {
              if (effectiveTarget) void entityMutations.setTargetRating(effectiveTarget, rating);
            },
            onExport: () => {
              if (!effectiveTarget) return;
              store.set(exportModalAtom, {
                open: true, fileCount: selCount, target: effectiveTarget,
              });
            },
            onRemoveFromFolder: () => { void removeSelectionFromCurrentFolder(effectiveTarget); },
            onOpenTagSelect: () => { setTagSelectModal({ open: true }); },
            onOpenAiTagger: canAutoTag
              ? () => { setAiTaggerPortal({ open: true, anchor: inspectorAnchor() }); }
              : undefined,
            onMoveToTrash: () => { void setSelectionLifecycle('trash', effectiveTarget); },
            onRestore: () => { void setSelectionLifecycle('active', effectiveTarget); },
            onPermanentDelete: () => { permanentlyDeleteSelection(effectiveTarget, selCount); },
            onAccept: () => { void setSelectionLifecycle('active', effectiveTarget); },
            onReject: () => { void setSelectionLifecycle('trash', effectiveTarget); },
          });
          contextMenu.openAt(pos, entries);
        }}
        onEmptyContextMenu={(pos) => {
          // Don't clear selection — let the menu reflect current state
          const entries = buildEmptyContextMenu({
            selectionCount,
            querySelectionActive,
            singleSelected: selectionCount === 1,
            singleHash: selectionCount === 1
              ? items.find((item) => selectedItemIds.has(item.item_id))?.display_file_hash ?? null
              : null,
            scopeKind: null,
            statusFilter: null,
            loadedCount: items.length,
            onSelectAll: () => selectAllResults(),
            onDeselectAll: () => clearSelection(),
          });
          contextMenu.openAt(pos, entries);
        }}
        onLoadMore={cursor ? () => gridController.loadNextPage() : undefined}
      />
    );
  };

  return (
    <div className={styles.root}>
      <ApplicationMenuButton />
      {renderIncomingSurface()}

      {fileDragOver && (
        <div className={styles.dropOverlay}>
          <div className={styles.dropOverlayBadge}>
            Drop files to import
            {gridScope.kind === 'folder' && <span className={styles.dropOverlaySub}>into current folder</span>}
          </div>
        </div>
      )}

      {viewerSession && (
        <MediaView
          items={items}
          currentIndex={resolveViewerIndex(viewerSession, items)}
          totalCount={totalCount}
          onNavigate={(delta) => {
            const next = navigateViewerSession(viewerSession, items, delta);
            if (next) {
              setViewerSession(next);
              dispatchSelection({ type: 'replace_items', itemIds: new Set([next.currentItemId]), anchor: next.currentItemId });
            }
          }}
          onClose={(exitItemId) => {
            setViewerSession(null);
            if (exitItemId != null) {
              dispatchSelection({ type: 'replace_items', itemIds: new Set([exitItemId]), anchor: exitItemId });
              const idx = items.findIndex((i) => i.item_id === exitItemId);
              scrollToItem(idx);
            }
          }}
          onLoadMore={cursor ? () => gridController.loadNextPage() : undefined}
        />
      )}

      {quickLookSession && (
        <QuickLook
          items={items}
          currentIndex={resolveViewerIndex(quickLookSession, items)}
          totalCount={totalCount}
          onNavigate={(delta) => {
            const next = navigateViewerSession(quickLookSession, items, delta);
            if (next) {
              setQuickLookSession(next);
              dispatchSelection({ type: 'replace_items', itemIds: new Set([next.currentItemId]), anchor: next.currentItemId });
              const idx = items.findIndex((item) => item.item_id === next.currentItemId);
              if (idx >= 0) {
                scrollToItem(idx, 'center');
              }
            }
          }}
          onClose={(exitItemId) => {
            setQuickLookSession(null);
            if (exitItemId != null) {
              dispatchSelection({ type: 'replace_items', itemIds: new Set([exitItemId]), anchor: exitItemId });
              const idx = items.findIndex((i) => i.item_id === exitItemId);
              scrollToItem(idx);
            }
          }}
          onLoadMore={cursor ? () => gridController.loadNextPage() : undefined}
        />
      )}

      {contextMenu.state && (
        <ContextMenu
          entries={contextMenu.state.entries}
          position={contextMenu.state.position}
          onClose={contextMenu.close}
        />
      )}

      <TagSelectPanel />
      <FolderPickerPanel />
      <AiTaggerPanel />
    </div>
  );
}
