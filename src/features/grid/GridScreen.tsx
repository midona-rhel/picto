/**
 * Grid screen — feature root. Reads state, delegates to CanvasGrid.
 */

import { useEffect, useRef, useState, useCallback } from 'react';
import { useAtomValue, useSetAtom, getDefaultStore } from 'jotai';
import { IconPhoto, IconUpload, IconFolderPlus } from '@tabler/icons-react';
import * as entityMutations from '../../controllers/entityMutations';
import { getShortcut, matchesShortcutDef } from '../../shared/lib/shortcuts';
import {
  activeNodeIdAtom,
  controlPreferencesAtom,
  inspectorCollapsedAtom,
  sidebarCollapsedAtom,
} from '../../state/navigation';
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
  gridShowItemCountAtom,
  gridShowResolutionAtom,
  gridFitThumbnailsAtom,
  gridGrayscaleAtom,
  gridSearchTextAtom,
  gridFiltersAtom,
  gridTotalCountAtom,
  gridTotalSizeBytesAtom,
  gridScopeAtom,
  gridChildFoldersAtom,
  gridFilterToolbarOpenAtom,
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
  displayedScopeLabelAtom,
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
import { buildTileContextMenu, buildEmptyContextMenu, buildEntityOpenContextEntries } from './gridContextMenu';
import { navigateToNode } from '../../state/navigationHistory';
import { viewerSessionAtom, quickLookSessionAtom, createViewerSession, navigateViewerSession, resolveViewerIndex } from '../../state/viewer';
import { aiTaggerPortalAtom, folderPickerPortalAtom, inspectorAnchor } from '../../state/portals';
import { groupOrganizerModalAtom, confirmModalAtom, folderImportModalAtom, exportModalAtom, batchRenameModalAtom, folderWatchModalAtom, tagSelectModalAtom, folderPickerModalAtom, smartFolderModalAtom } from '../../state/modals';
import { organizeIntoGroup, ungroup } from '../../platform/entityApi';
import { GroupSurface } from '../groups/GroupSurface';
import { MediaView } from '../viewer/MediaView';
import { GridQuickLook } from '../viewer/GridQuickLook';
import {
  InboxReviewControls,
  resolveInboxReviewItemId,
  type InboxReviewDecision,
} from '../viewer/InboxReviewControls';
import { useGridArrowNav } from './hooks/useGridArrowNav';
import type { LayoutResult } from './layout/types';
import { windowController } from '../../controllers/windowController';
import { tagsController } from '../../controllers/tagsController';
import { tagName } from '../tags/tagContextMenu';
import { chooseAndImportFiles, chooseAndImportFolder, filesController, hasClipboardImport, manualImportParamsForScope, openPictoPackImport, pasteImport, pictoPackPathFromDrop, requestMediaImport } from '../../controllers/filesController';
import { viewerController } from '../../controllers/viewerController';
import { EmptyState, EmptyStateAction } from '../../shared/ui/EmptyState';
import { scrollGridItemIntoView, type GridScrollAlignment } from './gridScroll';
import { resolveContextMenuTarget } from './gridMenuSelection';
import styles from './GridScreen.module.css';
import type { CanonicalEntityGridItem, EntityTarget, Lifecycle } from '../../shared/types/canonical';
import { showErrorNotification } from '../../shared/lib/notifications';
import { countActiveGridFilters, GridFilterToolbar } from './GridFilterMenu';
import { useShortcutScope } from '../../shared/hooks/useShortcutScope';
import { announceUndoableMutation } from '../../runtime/historyRuntime';
import { addQuickAccess, removeQuickAccess, reorderQuickAccess, useQuickAccess } from '../sidebar/quickAccessPreferences';
import { openCurrentLibraryCoverPicker } from '../library/libraryAppearance';
import { reverseImageSearch } from '../../platform/shellApi';
import {
  availableBulkFolderMoveTargets,
  availableFolderMoveTargets,
  buildBulkFolderContextMenu,
  buildFolderContextMenu,
  topLevelSelectedFolderIds,
} from '../folders/folderContextMenu';
import { ColorPicker } from '../../shared/ui/ColorPicker';
import { IconPicker } from '../../shared/ui/IconPicker';
import { compileGridQuery, createEmptyItemFilters, textSearchQuery } from '../../shared/lib/itemFilters';
import { readRecentFolderIds } from '../../shared/hooks/useRecentFolders';
import type { GridScrollPosition } from '../../shared/types/gridScroll';
import { pendingSidebarRenameNodeIdAtom } from '../../state/sidebar';
import { openFolderAutoTagsEditor } from '../folders/folderAutoTagsWorkflow';
import { t } from '../../i18n';

const store = getDefaultStore();
function supportsExplicitImageAutoTagging(
  querySelectionActive: boolean,
  itemIds: Set<number>,
  items: Array<{ root_id: number; mime: string }>,
): boolean {
  if (querySelectionActive || itemIds.size === 0) {
    return false;
  }
  const selectedItems = items.filter((item) => itemIds.has(item.root_id));
  return (
    selectedItems.length === itemIds.size &&
    selectedItems.every((item) => item.mime.startsWith('image/'))
  );
}

interface GridScreenProps {
  nodeId?: string;
  transitionPhase?: GridTransitionPhase;
  initialScrollPosition?: GridScrollPosition | null;
  onFirstPaint?: () => void;
  onScrollPositionChange?: (position: GridScrollPosition) => void;
}

export function GridScreen({
  nodeId,
  transitionPhase = 'idle',
  initialScrollPosition = null,
  onFirstPaint,
  onScrollPositionChange,
}: GridScreenProps) {
  const activeNodeId = useAtomValue(activeNodeIdAtom);
  const sidebarCollapsed = useAtomValue(sidebarCollapsedAtom);
  const inspectorCollapsed = useAtomValue(inspectorCollapsedAtom);
  const controlPreferences = useAtomValue(controlPreferencesAtom);
  const displayedNodeId = nodeId ?? activeNodeId;
  const items = useAtomValue(gridItemsAtom);
  const loading = useAtomValue(gridLoadingAtom);
  const error = useAtomValue(gridErrorAtom);
  const cursor = useAtomValue(gridCursorAtom);
  const viewMode = useAtomValue(gridViewModeAtom);
  const targetSize = useAtomValue(gridTargetSizeAtom);
  const showName = useAtomValue(gridShowNameAtom);
  const showExtension = useAtomValue(gridShowExtensionAtom);
  const showExtensionLabel = useAtomValue(gridShowExtensionLabelAtom);
  const showItemCount = useAtomValue(gridShowItemCountAtom);
  const showResolution = useAtomValue(gridShowResolutionAtom);
  const fitThumbnails = useAtomValue(gridFitThumbnailsAtom);
  const grayscale = useAtomValue(gridGrayscaleAtom);
  const searchText = useAtomValue(gridSearchTextAtom);
  const filters = useAtomValue(gridFiltersAtom);
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
  const viewerOpenRef = useRef(false);
  viewerOpenRef.current = Boolean(viewerSession || quickLookSession);
  const setAiTaggerPortal = useSetAtom(aiTaggerPortalAtom);
  const setFolderPortal = useSetAtom(folderPickerPortalAtom);

  const setTagSelectModal = useSetAtom(tagSelectModalAtom);
  const setFolderPickerModal = useSetAtom(folderPickerModalAtom);
  const setGroupOrganizerModal = useSetAtom(groupOrganizerModalAtom);
  const setPendingSidebarRenameNodeId = useSetAtom(pendingSidebarRenameNodeIdAtom);
  const setSmartFolderModal = useSetAtom(smartFolderModalAtom);
  const gridContainerRef = useRef<HTMLDivElement | null>(null);
  const gridLayoutRef = useRef<LayoutResult | null>(null);
  const [renamingIndex, setRenamingIndex] = useState<number | null>(null);
  const [renamingSubfolderId, setRenamingSubfolderId] = useState<string | null>(null);
  const [groupInitialMode, setGroupInitialMode] = useState<'reader' | 'editor'>('reader');
  const collectionBreadcrumbParent = useAtomValue(displayedScopeLabelAtom) || t('Collections');
  const subfolderGridRef = useRef<SubfolderGridHandle>(null);
  const quickAccessIds = useQuickAccess();

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

  useEffect(() => {
    if (viewerSession || quickLookSession) setFileDragOver(false);
  }, [quickLookSession, viewerSession]);
  const gridScopeRef2 = useRef(gridScope);
  gridScopeRef2.current = gridScope;

  useEffect(() => {
    const webview = (window as any).picto?.webview;
    if (!webview?.onDragDropEvent) return;

    const promise = webview.onDragDropEvent((event: { payload: {
      type: string;
      paths?: string[];
      temporaryPaths?: string[];
      sourceUrls?: string[];
      error?: string;
    } }) => {
      const { type, paths, temporaryPaths = [], sourceUrls = [], error: dropError } = event.payload;
      // Completely ignore all drag events while any app-originated drag is active
      if (isNativeDragPending() || isDragActiveCheck() || isInternalDragOrigin()) return;
      if (gridScopeRef2.current.kind === 'media_matches') {
        setFileDragOver(false);
        return;
      }
      if (viewerOpenRef.current) {
        setFileDragOver(false);
        return;
      }
      if (type === 'enter') { setFileDragOver(true); return; }
      if (type === 'leave') { setFileDragOver(false); return; }
      if (type !== 'drop') return;
      setFileDragOver(false);
      if (dropError) {
        showErrorNotification({ title: t("Could not import dropped media"), message: dropError });
        return;
      }
      if (!paths?.length) return;

      const scope = gridScopeRef2.current;
      const folderId = scope.kind === 'folder' ? scope.folder_id : null;
      const temporary = new Set(temporaryPaths);
      const browserPaths = paths.filter((path) => temporary.has(path));
      const localPaths = paths.filter((path) => !temporary.has(path));

      try {
        const pictoPackPath = pictoPackPathFromDrop(paths);
        if (pictoPackPath) {
          void openPictoPackImport(pictoPackPath).catch((reason) => {
            showErrorNotification({
              title: t("Could not import Picto Pack"),
              message: reason instanceof Error ? reason.message : String(reason),
            });
          });
          return;
        }
      } catch (reason) {
        showErrorNotification({
          title: t("Could not import Picto Pack"),
          message: reason instanceof Error ? reason.message : String(reason),
        });
        return;
      }

      // Detect folder drop (single path without media extension)
      const mediaExt = /\.(jpe?g|png|gif|webp|bmp|tiff?|svg|mp4|mkv|webm|avi|mov|wmv|flv|m4v|avif|jxl|ico|pdf)$/i;
      if (localPaths.length === 1 && browserPaths.length === 0 && !mediaExt.test(localPaths[0])) {
        // Show import modal for folder drops
        store.set(folderImportModalAtom, {
          open: true,
          path: localPaths[0],
          targetFolderId: folderId ?? null,
          lifecycle: manualImportParamsForScope(scope).lifecycle,
        });
        return;
      }
      if (localPaths.length > 0) {
        void requestMediaImport(localPaths, manualImportParamsForScope(scope,
          folderId != null ? { parent_folder_id: folderId } : {}));
      }
      if (browserPaths.length > 0) {
        void requestMediaImport(browserPaths, manualImportParamsForScope(scope, {
          ...(folderId != null ? { parent_folder_id: folderId } : {}),
          delete_after_ingest: true,
          source_urls: sourceUrls,
        }));
      }
    });

    return () => { promise.then((fn: () => void) => fn()); };
  }, []);

  const childFolders = useAtomValue(gridChildFoldersAtom);
  const filterToolbarOpen = useAtomValue(gridFilterToolbarOpenAtom);
  const viewportCommitKey = `${Number(filterToolbarOpen)}:${Number(sidebarCollapsed)}:${Number(inspectorCollapsed)}`;
  const contextMenu = useContextMenu();

  const setDisplayedGridSnapshot = useSetAtom(displayedGridSnapshotAtom);
  const setDisplayedInspectorTarget = useSetAtom(displayedInspectorTargetAtom);
  const setDisplayedEntityData = useSetAtom(displayedInspectorItemDetailsAtom);
  const setInspectorLoading = useSetAtom(inspectorLoadingAtom);
  const setInspectorError = useSetAtom(inspectorErrorAtom);
  const liveTarget = useAtomValue(liveInspectorTargetAtom);

  const lastScrollTopRef = useRef(0);

  // Commit the displayed scene while the replacement surface is hidden, before
  // the title, inspector, and grid begin their shared fade-in.
  // During idle: only commit data changes within the same scope.
  const displayedNodeIdRef = useRef(displayedNodeId);

  useEffect(() => {
    // During a transition: commit once replacement data is loaded.
    // During idle: only commit if we're on the same scope.
    const isSameScope = displayedNodeId === displayedNodeIdRef.current;
    const shouldCommit = transitionPhase === 'waiting'
      || transitionPhase === 'fading_in'
      || (transitionPhase === 'idle' && isSameScope);

    if (shouldCommit) {
      displayedNodeIdRef.current = displayedNodeId;
      setDisplayedGridSnapshot({
        nodeId: displayedNodeId,
        previewItems: items.slice(0, 4),
        totalCount,
        totalSizeBytes,
        searchText: searchText.trim(),
        filters,
        sidebarNode: sidebarNodes.find((n) => n.id === displayedNodeId) ?? null,
      });
      // Manager-owned grids keep their manager scope in the inspector. Commit
      // that target with the replacement grid instead of waiting for a click.
      const isSubfolderSelected = liveTarget.kind === 'scope' && 'nodeId' in liveTarget && liveTarget.nodeId !== displayedNodeId;
      if (transitionPhase !== 'idle' && liveTarget.kind === 'scope') {
        setDisplayedInspectorTarget(liveTarget);
        setDisplayedEntityData(null);
        setInspectorLoading(false);
        setInspectorError(null);
      } else if (!isSubfolderSelected && (liveTarget.kind === 'scope' || liveTarget.kind === 'none')) {
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
    filters,
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
    entityMutations.settleSelectionAfterMutation();
  }, [gridScope, selectionTarget]);

  const setSelectionLifecycle = useCallback(async (lifecycle: Lifecycle, target = selectionTarget) => {
    if (!target) return;
    await entityMutations.setTargetLifecycle(target, lifecycle);
  }, [selectionTarget]);

  const permanentlyDeleteSelection = useCallback((target = selectionTarget, count = selectionCount) => {
    if (!target) return;
    store.set(confirmModalAtom, {
      open: true,
      title: t("Delete Permanently"),
      message: `This will permanently delete ${count} item${count !== 1 ? 's' : ''}. This cannot be undone.`,
      confirmLabel: t("Delete"),
      danger: true,
      onConfirm: () => {
        void entityMutations.permanentlyDeleteTarget(target);
      },
    });
  }, [selectionTarget, selectionCount, clearSelection]);

  // ── Detail window communication ──
  // When a detail window opens, it sends 'detail-window-ready' with { hash }.
  // We respond with ONLY the selected images (not the entire grid).
  // Single selection → one image, no navigation in detail window.
  // Multi selection → those images as a navigable set.
  const detailWindowSelectionRef = useRef(new Map<string, number[]>());
  const wheelSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const wheelTargetSizeRef = useRef(targetSize);

  useEffect(() => {
    wheelTargetSizeRef.current = targetSize;
  }, [targetSize]);

  useEffect(() => () => {
    if (wheelSaveTimerRef.current) clearTimeout(wheelSaveTimerRef.current);
  }, []);
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
        .map((itemId: number) => curItems.find((i: any) => i.root_id === itemId))
        .filter(Boolean)
        .map((i: any) => ({
          item_id: i.root_id,
          hash: i.content_hash,
          name: i.name,
          mime: i.mime,
          width: i.width,
          height: i.height,
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
  const pendingDetailNavigationRef = useRef<number | null>(null);
  const gridScopeRef = useRef(gridScope);
  gridScopeRef.current = gridScope;

  // Refs for setters used in the keydown handler (avoid re-registering on every render)
  const setTagSelectModalRef = useRef(setTagSelectModal);
  setTagSelectModalRef.current = setTagSelectModal;
  const setFolderPickerModalRef = useRef(setFolderPickerModal);
  setFolderPickerModalRef.current = setFolderPickerModal;
  const setAiTaggerPortalRef = useRef(setAiTaggerPortal);
  setAiTaggerPortalRef.current = setAiTaggerPortal;

  const afterViewerItemReady = useCallback((
    item: CanonicalEntityGridItem,
    open: () => void,
  ) => {
    if (item.kind !== 'collection') {
      open();
      return;
    }
    void viewerController.prefetchItemDetails(item.root_id)
      .then((details) => {
        if (details.root.kind !== 'collection') throw new Error('The selected item is no longer a group.');
        open();
      })
      .catch((reason) => showErrorNotification({
        title: t("Could not open group"),
        message: reason instanceof Error ? reason.message : String(reason),
      }));
  }, []);

  const openGridItem = useCallback((
    item: CanonicalEntityGridItem,
    sourceItems: CanonicalEntityGridItem[],
    mode: 'reader' | 'editor' = 'reader',
  ) => {
    afterViewerItemReady(item, () => {
      setGroupInitialMode(item.kind === 'collection' ? mode : 'reader');
      setQuickLookSession(null);
      setViewerSession(createViewerSession(sourceItems, item.root_id));
    });
  }, [afterViewerItemReady, setQuickLookSession, setViewerSession]);

  const openGridItemInWindow = useCallback((
    item: CanonicalEntityGridItem,
    selectedItemIds: number[] = [item.root_id],
  ) => {
    if (item.kind === 'collection') {
      void windowController.openDetailWindow({ item_id: item.root_id });
      return;
    }
    if (!item.content_hash) return;
    const label = `detail-${item.content_hash.slice(0, 12)}`;
    detailWindowSelectionRef.current.set(label, selectedItemIds);
    void windowController.openDetailWindow({
      hash: item.content_hash,
      width: item.width ?? null,
      height: item.height ?? null,
    });
  }, []);

  const organizeSelection = useCallback(async (target: EntityTarget, coverRootId: number) => {
    try {
      const [summary, noteDraft] = await Promise.all([
        entityMutations.getTargetSelectionSummary(target),
        entityMutations.getTargetCollectionNoteDraft(target),
      ]);
      const groups = summary.collection_candidates;
      if (groups.length === 1 && noteDraft.source_count === 0) {
        clearSelection();
        const result = await organizeIntoGroup({
          target,
          cover_root_id: coverRootId,
          winning_collection_id: groups[0].collection_id,
          name: null,
          notes: null,
        });
        dispatchSelection({
          type: 'replace_items',
          itemIds: new Set([result.collection_id]),
          anchor: result.collection_id,
        });
        await announceUndoableMutation('collections.organize');
        return;
      }
      setGroupOrganizerModal({
        open: true,
        target,
        coverRootId,
        groups,
        notes: noteDraft.notes,
        notesMaximumBytes: noteDraft.maximum_bytes,
        onBeforeSubmit: clearSelection,
        onComplete: (groupId) => dispatchSelection({
          type: 'replace_items',
          itemIds: new Set([groupId]),
          anchor: groupId,
        }),
      });
    } catch (reason) {
      showErrorNotification({
        title: t("Could not create group"),
        message: reason instanceof Error ? reason.message : String(reason),
      });
    }
  }, [clearSelection, dispatchSelection, setGroupOrganizerModal]);

  useShortcutScope((e) => {
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
      grayscale:       getShortcut('view.grayscale')!,
      pasteImport:     getShortcut('edit.pasteImport')!,
      copy:           getShortcut('edit.copy')!,
      accept:         getShortcut('inbox.accept')!,
      reject:         getShortcut('inbox.reject')!,
    };

      // Don't handle grid shortcuts while a viewer is open — the viewer handles its own keys.
      // Exception: detailView/quicklook shortcuts to open viewers are checked below with their own guards.
      if (viewerSessionRef.current || quickLookSessionRef.current) {
        // Only allow viewer-opening shortcuts through (they have their own viewer-state guards)
        if (!matchesShortcutDef(e, defs.detailView) && !matchesShortcutDef(e, defs.quicklook)) return;
      }
      if (matchesShortcutDef(e, defs.grayscale)) {
        e.preventDefault();
        store.set(gridGrayscaleAtom, !store.get(gridGrayscaleAtom));
        return;
      }
      const count = selectionCountRef.current;
      const itemIds = selectedItemIdsRef.current;
      const curItems = itemsRef.current;
      const scope = gridScopeRef.current;
      const isTrash = scope.kind === 'trash';
      const singleItemId = count === 1 ? [...itemIds][0] : null;
      const singleItem = singleItemId == null
        ? null
        : curItems.find((item) => item.root_id === singleItemId) ?? null;
      const singleFileHash = singleItem?.content_hash ?? null;
      const canAutoTag = supportsExplicitImageAutoTagging(
        querySelectionActiveRef.current,
        itemIds,
        curItems,
      );

      if (matchesShortcutDef(e, defs.pasteImport)
        && scope.kind !== 'trash'
        && scope.kind !== 'smart_folder'
        && scope.kind !== 'recently_viewed'
        && scope.kind !== 'media_matches') {
        e.preventDefault();
        void pasteImport(scope).catch((reason) => showErrorNotification({
          title: t("Could not paste import"),
          message: reason instanceof Error ? reason.message : String(reason),
        }));
        return;
      }

      if (matchesShortcutDef(e, defs.copy) && count > 0 && !querySelectionActiveRef.current) {
        e.preventDefault();
        void filesController.copyTarget({ kind: 'explicit', root_ids: [...itemIds] })
          .catch((reason) => showErrorNotification({
            title: t("Could not copy selection"),
            message: reason instanceof Error ? reason.message : String(reason),
          }));
        return;
      }

      if (scope.kind === 'inbox' && count > 0 && matchesShortcutDef(e, defs.accept)) {
        e.preventDefault();
        void setSelectionLifecycle('active');
        return;
      }
      if (scope.kind === 'inbox' && count > 0 && matchesShortcutDef(e, defs.reject)) {
        e.preventDefault();
        void setSelectionLifecycle('trash');
        return;
      }

      if (matchesShortcutDef(e, defs.selectAll)) { e.preventDefault(); selectAllResults(); return; }
      if (matchesShortcutDef(e, defs.deselectAll) && count > 0) { e.preventDefault(); clearSelection(); return; }

      if (matchesShortcutDef(e, defs.detailView) && singleItemId != null && !viewerSessionRef.current && !quickLookSessionRef.current) {
        e.preventDefault();
        if (singleItem) openGridItem(singleItem, curItems);
        return;
      }
      if (matchesShortcutDef(e, defs.quicklook) && !viewerSessionRef.current) {
        if (controlPreferences.spaceKeyAction === 'scroll') {
          e.preventDefault();
          gridContainerRef.current?.scrollBy({
            top: (gridContainerRef.current.clientHeight || 0) * (e.shiftKey ? -0.9 : 0.9),
            behavior: 'smooth',
          });
          return;
        }
        e.preventDefault();
        if (quickLookSessionRef.current) setQuickLookSession(null);
        else if (singleItemId != null && singleItem) {
          afterViewerItemReady(singleItem, () => {
            setQuickLookSession(createViewerSession(curItems, singleItemId));
          });
        }
        return;
      }

      if (matchesShortcutDef(e, defs.openDefault) && singleFileHash && singleItem?.kind !== 'collection') {
        e.preventDefault(); void filesController.openDefaultAppForHash(singleFileHash); return;
      }
      if (matchesShortcutDef(e, defs.revealInFolder) && singleFileHash && singleItem?.kind !== 'collection') {
        e.preventDefault(); void filesController.revealHashInFolder(singleFileHash); return;
      }
      if (matchesShortcutDef(e, defs.openNewWindow) && count > 0) {
        e.preventDefault();
        const selectedArr = [...itemIds];
        const item = singleItem ?? curItems.find((candidate) => candidate.root_id === selectedArr[0]);
        if (!item) return;
        openGridItemInWindow(item, selectedArr);
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

      if (count > 0) {
        for (let digit = 0; digit <= 5; digit += 1) {
          const rating = getShortcut(`rate.${digit}`);
          if (rating && matchesShortcutDef(e, rating)) {
          e.preventDefault();
          void entityMutations.setTargetRating(
            { kind: 'explicit', root_ids: [...itemIds] },
            digit,
          );
          return;
          }
        }
      }
  }, { priority: 20 });

  const isEmpty = items.length === 0 && totalCount === 0;

  useEffect(() => {
    const hasSubfolders = !viewerSession
      && childFolders.length > 0 && gridScope.kind === 'folder';
    const staticSurfaceCommitted = Boolean(error) || (isEmpty && !hasSubfolders);
    if (transitionPhase === 'waiting' && !loading && staticSurfaceCommitted) onFirstPaint?.();
  }, [childFolders.length, error, gridScope.kind, isEmpty, loading, onFirstPaint, transitionPhase, viewerSession]);
  const viewerIndex = viewerSession ? resolveViewerIndex(viewerSession, items) : -1;
  const viewerItem = viewerIndex >= 0 ? items[viewerIndex] ?? null : null;
  const quickLookIndex = quickLookSession ? resolveViewerIndex(quickLookSession, items) : -1;
  const quickLookItem = quickLookIndex >= 0 ? items[quickLookIndex] ?? null : null;
  const inboxReviewItemId = resolveInboxReviewItemId(viewerItem, quickLookItem);
  const inboxReviewNextItemIdRef = useRef<number | null>(null);
  const inboxReviewModeRef = useRef<'detail' | 'quicklook'>('detail');

  const commitInboxReview = useCallback(async (itemId: number, decision: InboxReviewDecision) => {
    const currentItems = itemsRef.current;
    const currentIndex = currentItems.findIndex((item) => item.root_id === itemId);
    inboxReviewNextItemIdRef.current = currentIndex < 0
      ? null
      : currentItems[currentIndex + 1]?.root_id ?? currentItems[currentIndex - 1]?.root_id ?? null;
    inboxReviewModeRef.current = quickLookSessionRef.current ? 'quicklook' : 'detail';
    await entityMutations.setTargetLifecycle(
      { kind: 'explicit', root_ids: [itemId] },
      decision === 'accept' ? 'active' : 'trash',
    );
  }, []);

  const navigateQuickLook = useCallback((delta: number) => {
    if (!quickLookSession) return;
    const next = navigateViewerSession(quickLookSession, items, delta);
    if (!next) return;
    const nextItem = items.find((item) => item.root_id === next.currentItemId);
    if (!nextItem) return;
    afterViewerItemReady(nextItem, () => {
      setQuickLookSession(next);
      dispatchSelection({ type: 'replace_items', itemIds: new Set([next.currentItemId]), anchor: next.currentItemId });
      const index = items.findIndex((item) => item.root_id === next.currentItemId);
      if (index >= 0) scrollToItem(index, 'center');
    });
  }, [afterViewerItemReady, dispatchSelection, items, quickLookSession, scrollToItem, setQuickLookSession]);

  const closeQuickLook = useCallback((exitItemId?: number) => {
    setQuickLookSession(null);
    if (exitItemId == null) return;
    dispatchSelection({ type: 'replace_items', itemIds: new Set([exitItemId]), anchor: exitItemId });
    scrollToItem(items.findIndex((item) => item.root_id === exitItemId));
  }, [dispatchSelection, items, scrollToItem, setQuickLookSession]);

  const navigateRootDetail = useCallback((delta: number) => {
    if (!viewerSession) return;
    const next = navigateViewerSession(viewerSession, items, delta);
    if (!next) {
      if (delta > 0 && cursor && pendingDetailNavigationRef.current == null) {
        const anchorItemId = viewerSession.currentItemId;
        pendingDetailNavigationRef.current = anchorItemId;
        void gridController.loadNextPage().catch(() => {
          pendingDetailNavigationRef.current = null;
        });
      }
      return;
    }
    const nextItem = items.find((item) => item.root_id === next.currentItemId);
    if (!nextItem) return;
    afterViewerItemReady(nextItem, () => {
      pendingDetailNavigationRef.current = null;
      setGroupInitialMode('reader');
      setViewerSession(next);
      dispatchSelection({ type: 'replace_items', itemIds: new Set([next.currentItemId]), anchor: next.currentItemId });
    });
  }, [afterViewerItemReady, cursor, dispatchSelection, items, setViewerSession, viewerSession]);

  useEffect(() => {
    const anchorItemId = pendingDetailNavigationRef.current;
    if (anchorItemId == null || !viewerSession || viewerSession.currentItemId !== anchorItemId) return;
    const loadedNext = navigateViewerSession(viewerSession, items, 1);
    if (!loadedNext) {
      if (!cursor) pendingDetailNavigationRef.current = null;
      return;
    }
    const nextItem = items.find((item) => item.root_id === loadedNext.currentItemId);
    if (!nextItem) return;
    afterViewerItemReady(nextItem, () => {
      pendingDetailNavigationRef.current = null;
      setGroupInitialMode('reader');
      setViewerSession(loadedNext);
      dispatchSelection({
        type: 'replace_items',
        itemIds: new Set([loadedNext.currentItemId]),
        anchor: loadedNext.currentItemId,
      });
    });
  }, [afterViewerItemReady, cursor, dispatchSelection, items, setViewerSession, viewerSession]);

  const closeRootDetail = useCallback((exitItemId?: number) => {
    pendingDetailNavigationRef.current = null;
    setViewerSession(null);
    setGroupInitialMode('reader');
    if (exitItemId == null) return;
    dispatchSelection({ type: 'replace_items', itemIds: new Set([exitItemId]), anchor: exitItemId });
    scrollToItem(items.findIndex((item) => item.root_id === exitItemId));
  }, [dispatchSelection, items, scrollToItem, setViewerSession]);

  const advanceAfterInboxReview = useCallback(() => {
    const nextItemId = inboxReviewNextItemIdRef.current;
    inboxReviewNextItemIdRef.current = null;
    if (nextItemId == null) {
      if (inboxReviewModeRef.current === 'quicklook') closeQuickLook();
      else closeRootDetail();
      return;
    }

    const next = createViewerSession(itemsRef.current, nextItemId);
    if (!next) {
      if (inboxReviewModeRef.current === 'quicklook') closeQuickLook();
      else closeRootDetail();
      return;
    }

    const nextItem = itemsRef.current.find((item) => item.root_id === next.currentItemId);
    if (!nextItem) return;
    afterViewerItemReady(nextItem, () => {
      if (inboxReviewModeRef.current === 'quicklook') setQuickLookSession(next);
      else {
        setGroupInitialMode('reader');
        setViewerSession(next);
      }
      dispatchSelection({
        type: 'replace_items',
        itemIds: new Set([next.currentItemId]),
        anchor: next.currentItemId,
      });
    });
  }, [afterViewerItemReady, closeQuickLook, closeRootDetail, dispatchSelection, setQuickLookSession, setViewerSession]);

  const openEmptyGridContextMenu = useCallback(async (pos: { x: number; y: number }) => {
    const canImport = gridScope.kind !== 'trash'
      && gridScope.kind !== 'smart_folder'
      && gridScope.kind !== 'recently_viewed'
      && gridScope.kind !== 'media_matches';
    const clipboardImportAvailable = canImport ? await hasClipboardImport().catch(() => false) : false;
    const runImport = (operation: () => Promise<void>, title: string) => {
      void operation().catch((reason) => showErrorNotification({
        title,
        message: reason instanceof Error ? reason.message : String(reason),
      }));
    };
    const entries = buildEmptyContextMenu({
      selectionCount,
      querySelectionActive,
      singleSelected: selectionCount === 1,
      singleHash: null,
      scopeKind: gridScope.kind === 'folder'
        ? 'folder'
        : gridScope.kind === 'smart_folder' ? 'smart_folder' : 'system',
      statusFilter: null,
      loadedCount: items.length,
      onNewFolder: () => {
        const parentId = gridScope.kind === 'folder' ? gridScope.folder_id : null;
        void foldersController.create('New Folder', parentId)
          .then(setPendingSidebarRenameNodeId)
          .catch((reason) => showErrorNotification({
            title: t("Could not create folder"),
            message: reason instanceof Error ? reason.message : String(reason),
          }));
      },
      onNewSmartFolder: () => setSmartFolderModal({
        open: true,
        mode: 'create',
        initial: { name: 'New Smart Folder', view: { filter: { kind: 'all', value: [] }, sort: { field: 'imported_at', direction: 'descending', random_seed: null } } },
      }),
      onImportFiles: canImport
        ? () => runImport(() => chooseAndImportFiles(gridScope), t('Could not import files'))
        : undefined,
      onImportFolder: canImport
        ? () => runImport(() => chooseAndImportFolder(gridScope), t('Could not import folder'))
        : undefined,
      onPasteImport: clipboardImportAvailable
        ? () => runImport(() => pasteImport(gridScope), t('Could not paste import'))
        : undefined,
      onSortContents: gridScope.kind === 'folder'
        ? (field) => { void foldersController.sortContents(gridScope.folder_id, field); }
        : undefined,
    });
    contextMenu.openAt(pos, entries);
  }, [
    clearSelection,
    contextMenu,
    gridScope,
    items.length,
    querySelectionActive,
    selectAllResults,
    selectionCount,
    setPendingSidebarRenameNodeId,
    setSmartFolderModal,
  ]);

  const renderIncomingSurface = () => {
    // Quick Look overlays the settled grid; changing header geometry here would
    // trigger a needless canvas resize and scroll-anchor settlement.
    const hasSubfolders = !viewerSession
      && childFolders.length > 0 && gridScope.kind === 'folder';

    if (error) {
      return (
        <div className={styles.error}>
          <span>{error}</span>
          <button className={styles.retryBtn} onClick={() => gridController.loadFirstPage()}>
            {t("Retry")}</button>
        </div>
      );
    }

    if (isEmpty && !hasSubfolders) {
      const scopeKey = gridScope.kind;
      const hasSearch = textSearchQuery(searchText) != null;
      const hasFilters = countActiveGridFilters(filters) > 0;
      const emptyTitle = hasSearch ? 'No results found'
        : hasFilters ? t("No items match these filters")
        : scopeKey === 'inbox' ? 'Inbox is empty'
        : scopeKey === 'uncategorized' ? 'No uncategorized images'
        : scopeKey === 'untagged' ? 'No untagged images'
        : scopeKey === 'smart_folder' ? 'No matching images'
        : scopeKey === 'media_matches' ? 'No items use this media'
        : scopeKey === 'folder' ? 'This folder is empty'
        : 'No images';
      const emptyDesc = hasSearch ? 'Try different search terms or clear filters'
        : hasFilters ? t("Try adjusting or clearing your filters")
        : scopeKey === 'inbox' ? 'Run subscriptions to add new images to your inbox'
        : scopeKey === 'uncategorized' ? 'All your images are already assigned to folders'
        : scopeKey === 'untagged' ? 'All your images have been tagged'
        : scopeKey === 'smart_folder' ? 'Try adjusting the rules for this smart folder'
        : scopeKey === 'media_matches' ? 'This media is no longer used by another library item'
        : scopeKey === 'folder' ? 'Drag and drop files here, or import them below'
        : 'Drag and drop files here, or click the button below to import';
      const showImport = !hasSearch && !hasFilters
        && scopeKey !== 'inbox'
        && scopeKey !== 'untagged'
        && scopeKey !== 'smart_folder'
        && scopeKey !== 'media_matches';

      return (
        <div
          className={styles.emptyContextSurface}
          onContextMenu={(event) => {
            event.preventDefault();
            void openEmptyGridContextMenu({ x: event.clientX, y: event.clientY });
          }}
        >
        <EmptyState
          icon={<IconPhoto size={28} stroke={1.2} style={{ color: 'var(--color-bg-app)', opacity: 1 }} />}
          title={emptyTitle}
          description={emptyDesc}
          actions={showImport ? (
            <>
              <EmptyStateAction onClick={() => {
                void chooseAndImportFiles(gridScope).catch((err) => {
                  console.error('[grid] import files failed:', err);
                });
              }}>
                <IconUpload size={14} stroke={1.5} />
                {t("Import Files")}</EmptyStateAction>
              <EmptyStateAction onClick={() => {
                void chooseAndImportFolder(gridScope).catch((err) => {
                  console.error('[grid] import folder failed:', err);
                });
              }}>
                <IconFolderPlus size={14} stroke={1.5} />
                {t("Import Folder")}</EmptyStateAction>
            </>
          ) : undefined}
        />
        </div>
      );
    }

    const subfolderHeader = hasSubfolders ? (
      <SubfolderGrid
        ref={subfolderGridRef}
        childFolders={childFolders}
        targetSize={targetSize}
        totalImageCount={totalCount ?? items.length}
        selectedNodeIds={selectedSubfolderNodeIds}
        renamingNodeId={renamingSubfolderId}
        onRenameFolder={(nodeId, name) => {
          const folderId = Number.parseInt(nodeId.slice('folder:'.length), 10);
          setRenamingSubfolderId(null);
          if (Number.isNaN(folderId)) return;
          void foldersController.rename(folderId, name).catch((reason) => showErrorNotification({
            title: t("Unable to rename folder"),
            message: reason instanceof Error ? reason.message : String(reason),
          }));
        }}
        onCancelRename={() => setRenamingSubfolderId(null)}
        onOpenFolder={(nodeId) => {
          navigateToNode(nodeId);
        }}
        onSelectFolder={(nodeId, event) => {
          if (event.metaKey || event.ctrlKey) {
            dispatchSelection({ type: 'toggle_folder', id: nodeId });
          } else {
            dispatchSelection({ type: 'replace_folders', ids: new Set([nodeId]), anchor: nodeId });
          }
        }}
        onFolderContextMenu={(nodeId, folder, pos) => {
          // Select the folder if not already selected
          if (!selectedSubfolderNodeIds.has(nodeId)) {
            dispatchSelection({ type: 'replace_folders', ids: new Set([nodeId]), anchor: nodeId });
          }
          const folderId = parseInt(nodeId.replace('folder:', ''), 10);
          if (isNaN(folderId)) return;
          const selectedFolderIds = selectedSubfolderNodeIds.has(nodeId)
            ? [...selectedSubfolderNodeIds]
              .map((id) => Number.parseInt(id.slice('folder:'.length), 10))
              .filter((id) => !Number.isNaN(id))
            : [folderId];
          if (selectedFolderIds.length > 1) {
            const selectedNodeIds = selectedFolderIds.map((id) => `folder:${id}`);
            const allInQuickAccess = selectedNodeIds.every((id) => quickAccessIds.includes(id));
            const movingFolderIds = topLevelSelectedFolderIds(sidebarNodes, selectedFolderIds);
            const movingNodeIds = movingFolderIds.map((id) => `folder:${id}`);
            const movingNodes = sidebarNodes.filter((candidate) => movingNodeIds.includes(candidate.id));
            const parentIds = movingNodes.map((candidate) => candidate.parent_id ?? null);
            const sharedParent = parentIds.every((parentId) => parentId === parentIds[0])
              && parentIds[0]?.startsWith('folder:')
              ? Number.parseInt(parentIds[0].slice(7), 10)
              : null;
            const entries = buildBulkFolderContextMenu({
              allInQuickAccess,
              count: selectedFolderIds.length,
              onToggleQuickAccess: () => {
                void reorderQuickAccess(allInQuickAccess
                  ? quickAccessIds.filter((id) => !selectedNodeIds.includes(id))
                  : [...new Set([...quickAccessIds, ...selectedNodeIds])]);
              },
              onDuplicate: () => {
                void Promise.all(selectedFolderIds.map((id) => foldersController.duplicate(id)));
              },
              onMove: () => {
                setFolderPortal({
                  open: true,
                  anchor: pos,
                  selectedFolderIds: sharedParent == null || Number.isNaN(sharedParent) ? [] : [sharedParent],
                  availableFolderIds: availableBulkFolderMoveTargets(sidebarNodes, movingFolderIds),
                  onApplyFolderParent: (parentId) => {
                    void foldersController.moveMany(movingFolderIds, parentId, []);
                  },
                });
              },
              onSetAutoTags: () => {
                void openFolderAutoTagsEditor(selectedFolderIds);
              },
              onSortContents: (field) => {
                void Promise.all(selectedFolderIds.map((id) => foldersController.sortContents(id, field)));
              },
              onDelete: () => {
                store.set(confirmModalAtom, {
                  open: true,
                  title: t("Delete Folders"),
                  danger: true,
                  confirmLabel: t("Delete"),
                  message: `Delete ${selectedFolderIds.length} selected folders and all their subfolders? Media inside these folders will remain untouched.`,
                  onConfirm: () => { void foldersController.deleteMany(selectedFolderIds); },
                });
              },
            });
            contextMenu.openAt(pos, entries);
            return;
          }
          const watchEnabled = Boolean((folder.meta as Record<string, unknown> | null)?.watch_enabled);
          const entries = buildFolderContextMenu({
            inQuickAccess: quickAccessIds.includes(nodeId),
            watchEnabled,
            onOpen: () => {
              navigateToNode(nodeId);
            },
            onNewSubfolder: () => {
              void foldersController.create('New Folder', folderId).then(setRenamingSubfolderId);
            },
            onToggleQuickAccess: () => {
              void (quickAccessIds.includes(nodeId) ? removeQuickAccess(nodeId) : addQuickAccess(nodeId));
            },
            onRename: () => setRenamingSubfolderId(nodeId),
            onMove: () => {
              const currentParentId = folder.parent_id?.startsWith('folder:')
                ? Number.parseInt(folder.parent_id.slice(7), 10)
                : null;
              setFolderPortal({
                open: true,
                anchor: pos,
                selectedFolderIds: currentParentId == null || Number.isNaN(currentParentId) ? [] : [currentParentId],
                availableFolderIds: availableFolderMoveTargets(sidebarNodes, folderId),
                onApplyFolderParent: (parentId) => { void foldersController.move(folderId, parentId, []); },
              });
            },
            onDuplicate: () => {
              void foldersController.duplicate(folderId).then(setRenamingSubfolderId);
            },
            onSetAutoTags: () => {
              void openFolderAutoTagsEditor([folderId]);
            },
            onImport: () => {
              void (async () => {
                const result = await (window as any).picto.dialog.open({
                  properties: ['openDirectory'],
                  multiple: false,
                  title: t("Import folder into {value0}", { value0: folder.name }),
                });
                if (!result) return;
                const path = typeof result === 'string' ? result : result[0];
                if (path) await foldersController.addMedia(path, folderId);
              })().catch((reason) => showErrorNotification({
                title: t("Unable to import folder"),
                message: reason instanceof Error ? reason.message : String(reason),
              }));
            },
            onAttachWatch: () => {
              store.set(folderWatchModalAtom, { open: true, folderId, initial: {} });
            },
            onRemoveWatch: watchEnabled ? () => {
              store.set(confirmModalAtom, {
                open: true,
                title: t("Remove Watch"),
                danger: true,
                confirmLabel: t("Remove"),
                message: `Stop watching the folder for "${folder.name}"?`,
                onConfirm: () => { void foldersController.clearWatchConfig(folderId); },
              });
            } : undefined,
            onSortTree: (descending, recursive) => {
              void foldersController.sortTree(folderId, descending, recursive);
            },
            onSortContents: (field) => { void foldersController.sortContents(folderId, field); },
            iconPickerEntry: {
              custom: true,
              key: 'folder-icon',
              render: () => (
                <IconPicker
                  compact
                  value={folder.icon ?? null}
                  onChange={(icon) => { void foldersController.applyIcon(folderId, icon); }}
                />
              ),
            },
            colorPickerEntry: {
              custom: true,
              key: 'folder-color',
              render: () => (
                <ColorPicker
                  value={folder.color ?? null}
                  onChange={(color) => foldersController.applyColor(folderId, color)}
                />
              ),
            },
            onExport: () => {
              store.set(exportModalAtom, {
                open: true,
                fileCount: folder.count ?? 0,
                target: {
                  kind: 'query',
                  query: compileGridQuery(
                    { kind: 'folder', folder_id: folderId },
                    createEmptyItemFilters(),
                    { field: 'imported_at', direction: 'descending', random_seed: null },
                  ),
                  excluded_root_ids: [],
                },
              });
            },
            onExportPictoPack: () => {
              filesController.requestPictoPackExport(
                { kind: 'folder', folder_id: folderId },
                folder.count ?? 0,
                folder.name,
              );
            },
            onDelete: () => {
              store.set(confirmModalAtom, {
                open: true,
                title: t("Delete Folder"),
                danger: true,
                confirmLabel: t("Delete"),
                message: `Delete "${folder.name}" and all its subfolders? Media inside these folders will remain untouched.`,
                onConfirm: () => { void foldersController.delete(folderId); },
              });
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
        viewportCommitKey={viewportCommitKey}
        viewMode={viewMode}
        targetSize={targetSize}
        showName={showName}
        showExtension={showExtension}
        showExtensionLabel={showExtensionLabel}
        showItemCount={showItemCount}
        showResolution={showResolution}
        fitThumbnails={fitThumbnails}
        grayscale={grayscale}
        totalCount={totalCount}
        interactive={!viewerSession && !quickLookSession}
        selectedItemIds={selectedItemIds}
        selectedFolderNodeIds={selectedSubfolderNodeIds}
        initialScrollPosition={initialScrollPosition}
        onContainerRef={(el) => { gridContainerRef.current = el; }}
        onLayoutChange={(l) => { gridLayoutRef.current = l; }}
        renamingIndex={renamingIndex}
        onRenameCommit={(idx, name) => {
          setRenamingIndex(null);
          const item = items[idx];
          if (item && name) void entityMutations.setItemName(item.root_id, name);
        }}
        onRenameCancel={() => setRenamingIndex(null)}
        onFirstPaint={loading ? undefined : onFirstPaint}
        onScrollPositionChange={(position) => {
          lastScrollTopRef.current = position.scrollTop;
          onScrollPositionChange?.(position);
        }}
        onTileClick={(index, item, event) => {
          const itemId = item.root_id;
          if (event?.shiftKey && selection.anchor?.kind === 'item') {
            const anchorIndex = items.findIndex((entry) => entry.root_id === selection.anchor!.id);
            const from = Math.min(anchorIndex >= 0 ? anchorIndex : index, index);
            const to = Math.max(anchorIndex >= 0 ? anchorIndex : index, index);
            const base = (event.metaKey || event.ctrlKey)
              ? new Set(selectedItemIds)
              : new Set<number>();
            for (let i = from; i <= to; i++) {
              if (items[i]) base.add(items[i].root_id);
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
          if (controlPreferences.gridDoubleClickAction === 'external' && item.kind === 'media') {
            void filesController.openDefaultAppForHash(item.content_hash);
          } else {
            openGridItem(item, items);
          }
        }}
        onTileMiddleClick={(_index, item) => {
          if (controlPreferences.gridMiddleClickAction === 'new_window') openGridItemInWindow(item);
        }}
        onGridWheel={(event) => {
          const action = controlPreferences.gridWheelAction;
          if (action !== 'zoom' || event.deltaY === 0) return;
          event.preventDefault();
          const currentTargetSize = wheelTargetSizeRef.current;
          const candidate = currentTargetSize + (event.deltaY < 0 ? 25 : -25);
          const nextTargetSize = Math.max(150, Math.min(900,
            candidate === currentTargetSize
              ? currentTargetSize + (event.deltaY < 0 ? 1 : -1)
              : candidate,
          ));
          wheelTargetSizeRef.current = nextTargetSize;
          gridController.updateView({ targetSize: nextTargetSize });
          if (wheelSaveTimerRef.current) clearTimeout(wheelSaveTimerRef.current);
          wheelSaveTimerRef.current = setTimeout(() => {
            wheelSaveTimerRef.current = null;
            gridController.saveViewPref({ target_size: nextTargetSize });
          }, 150);
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
          if (!selectedItemIds.has(item.root_id)) {
            effectiveItemIds = new Set([item.root_id]);
            dispatchSelection({ type: 'replace_items', itemIds: effectiveItemIds, anchor: item.root_id });
            effectiveSelectionMode = 'explicit';
            effectiveSelectionCount = 1;
            effectiveQuerySelectionActive = false;
          }

          // Derive context for menu builder
          const selCount = effectiveSelectionCount;
          const selectedItems = items.filter((it) => effectiveItemIds.has(it.root_id));
          const singleItem = effectiveSelectionMode === 'explicit'
            && selCount === 1
            && effectiveItemIds.has(item.root_id)
            ? item
            : null;
          const canAutoTag = effectiveSelectionMode === 'explicit'
            && selectedItems.length === effectiveItemIds.size
            && selectedItems.every((selected) => selected.kind === 'media')
            && selectedItems.every((selected) => selected.mime.startsWith('image/'));
          const containsGroup = selectedItems.some((selected) => selected.kind === 'collection');
          const effectiveTarget = resolveContextMenuTarget(
            effectiveQuerySelectionActive,
            selectionTarget,
            effectiveItemIds,
          );
          const scopeKind = gridScope.kind === 'folder' ? 'folder'
            : gridScope.kind === 'smart_folder' ? 'smart_folder'
            : 'system';
          const selectionLifecycle = selectedItems.length > 0
            && selectedItems.every((selected) => selected.lifecycle === selectedItems[0]?.lifecycle)
            ? selectedItems[0]?.lifecycle ?? null
            : null;
          const statusFilter = selectionLifecycle === 'inbox' || gridScope.kind === 'inbox' ? 'inbox'
            : selectionLifecycle === 'trash' || gridScope.kind === 'trash' ? 'trash'
            : gridScope.kind === 'all' ? 'active'
            : null;
          const lastUsedFolder = readRecentFolderIds()
            .map((folderId) => sidebarNodes.find((node) => node.id === `folder:${folderId}`))
            .find((node) => node != null);
          const canOpenWith = singleItem?.kind === 'media' && Boolean(singleItem.content_hash);
          const isMac = /Mac|iPhone|iPad/.test(navigator.platform);
          const isWindows = /Win/.test(navigator.platform);
          const openWithPending = canOpenWith && isMac;
          const openWithOptions = canOpenWith && isWindows
            ? { mode: 'chooser' as const, applications: [] }
            : null;
          const onOpenWithApplication = (hash: string, applicationPath: string) => {
            void filesController.openWithApplicationForHash(hash, applicationPath).catch((reason) => {
              showErrorNotification({
                title: t("Could not open file"),
                message: reason instanceof Error ? reason.message : String(reason),
              });
            });
          };
          const onOpenWithChooser = (hash: string) => {
            void filesController.openWithChooserForHash(hash).catch((reason) => {
              showErrorNotification({
                title: t("Could not open application chooser"),
                message: reason instanceof Error ? reason.message : String(reason),
              });
            });
          };

          const entries = buildTileContextMenu({
            selectionCount: selCount,
            querySelectionActive: effectiveQuerySelectionActive,
            aiTagEnabled: canAutoTag,
            singleSelected: effectiveSelectionMode === 'explicit' && selCount === 1,
            singleHash: singleItem?.content_hash ?? null,
            singleItemId: singleItem?.root_id ?? null,
            singleKind: singleItem?.kind ?? null,
            containsGroup,
            scopeKind,
            statusFilter,
            loadedCount: items.length,
            grayscale,
            onSortContents: !viewerSession && !quickLookSession && gridScope.kind === 'folder'
              ? (field) => { void foldersController.sortContents(gridScope.folder_id, field); }
              : undefined,
            onToggleGrayscale: () => store.set(gridGrayscaleAtom, !store.get(gridGrayscaleAtom)),
            onOpen: singleItem ? () => openGridItem(singleItem, items) : undefined,
            onOpenNewWindow: singleItem
              ? () => openGridItemInWindow(singleItem, [...effectiveItemIds])
              : undefined,
            onOpenDefault: (hash) => { void filesController.openDefaultAppForHash(hash); },
            openWithOptions,
            openWithPending,
            onOpenWithApplication,
            onOpenWithChooser,
            onRevealInFolder: (hash) => { void filesController.revealHashInFolder(hash); },
            onCopyFilePath: (hash) => { void filesController.copyFilePath(hash); },
            onCopyFile: (hash) => { void filesController.copyFileForHash(hash); },
            onCopySelection: effectiveSelectionMode === 'explicit' && effectiveTarget
              ? () => { void filesController.copyTarget(effectiveTarget); }
              : undefined,
            onCopySelectionPaths: effectiveSelectionMode === 'explicit' && effectiveTarget
              ? () => { void filesController.copyTargetPaths(effectiveTarget); }
              : undefined,
            onCopySelectionNames: effectiveSelectionMode === 'explicit' && selectedItems.length > 0
              ? () => filesController.copyText(selectedItems.map((selected) => selected.name ?? 'Untitled').join('\n'))
              : undefined,
            onCopySelectionLinks: effectiveSelectionMode === 'explicit' && effectiveTarget
              ? () => { void filesController.copyTargetLinks(effectiveTarget); }
              : undefined,
            onCopyName: (name) => { filesController.copyText(name); },
            singleName: singleItem?.name ?? null,
            singleMime: singleItem?.mime ?? null,
            onFindMediaMatches: (itemId) => navigateToNode(`media-matches:${itemId}`),
            onCopyLink: (link) => filesController.copyText(link),
            onRename: singleItem ? () => {
              const idx = items.findIndex((i) => i.root_id === singleItem.root_id);
              if (idx >= 0) setRenamingIndex(idx);
            } : undefined,
            onBatchRename: effectiveSelectionMode === 'explicit' && selectedItems.length > 1
              ? () => store.set(batchRenameModalAtom, {
                  open: true,
                  items: selectedItems.map((item) => ({ root_id: item.root_id, name: item.name ?? 'Untitled' })),
                })
              : undefined,
            onOrganizeGroup: effectiveTarget && selCount > 1
              ? () => { void organizeSelection(effectiveTarget, selectedItems[0].root_id); }
              : undefined,
            onEditGroup: singleItem?.kind === 'collection'
              ? () => openGridItem(singleItem, items, 'editor')
              : undefined,
            onUngroup: singleItem?.kind === 'collection'
              ? () => {
                  store.set(confirmModalAtom, {
                    open: true,
                    title: t("Ungroup?"),
                    message: 'Every member will return to the library as a separate item. Files and metadata will not be deleted.',
                    confirmLabel: t("Ungroup"),
                    onConfirm: () => {
                      void ungroup(singleItem.root_id)
                        .then(() => announceUndoableMutation('collections.ungroup'))
                        .then(() => clearSelection())
                        .catch((reason) => showErrorNotification({
                          title: t("Could not ungroup items"),
                          message: reason instanceof Error ? reason.message : String(reason),
                        }));
                    },
                  });
                }
              : undefined,
            onRegenerateThumbnails: () => {
              const hashes = selectedItems.map((selected) => selected.content_hash);
              void filesController.regenerateThumbnailsBatch(hashes);
            },
            onSetLibraryCover: (hash) => {
              void openCurrentLibraryCoverPicker(singleItem ? {
                media_item_id: singleItem.root_id,
                file_hash: hash,
                name: singleItem.name,
                pixel_width: singleItem.width,
                pixel_height: singleItem.height,
                mime_type: singleItem.mime,
              } : null).catch((reason) => showErrorNotification({
                title: t("Could not set library cover"),
                message: reason instanceof Error ? reason.message : String(reason),
              }));
            },
            onSetFolderCover: gridScope.kind === 'folder' && singleItem
              ? () => {
                  void foldersController.setCover(gridScope.folder_id, singleItem.root_id)
                    .catch((reason) => showErrorNotification({
                      title: t("Could not set folder cover"),
                      message: reason instanceof Error ? reason.message : String(reason),
                    }));
                }
              : undefined,
            onCopyTags: () => {
              if (!effectiveTarget) return;
              const tags = singleItem
                ? viewerController.getItemDetails(singleItem.root_id)
                  .then((details) => tagsController.getById(details.tag_ids))
                  .then((records) => records.map(tagName))
                : entityMutations.getTargetSelectionSummary(effectiveTarget)
                  .then((summary) => tagsController.getById(summary.shared_tags))
                  .then((records) => records.map(tagName));
              void tags.then((tagStrings) => {
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
            lastUsedFolderName: lastUsedFolder?.name ?? null,
            onAddToLastUsedFolder: effectiveTarget && lastUsedFolder ? () => {
              const folderId = Number.parseInt(lastUsedFolder.id.slice('folder:'.length), 10);
              if (Number.isNaN(folderId)) return;
              void entityMutations.updateTargetFolderMembership(effectiveTarget, folderId, 'add')
                .then(() => entityMutations.settleSelectionAfterMutation())
                .catch((reason) => showErrorNotification({
                  title: t('Could not add to {value0}', { value0: lastUsedFolder.name }),
                  message: reason instanceof Error ? reason.message : String(reason),
                }));
            } : undefined,
            onNewFolderWithSelection: effectiveTarget ? () => {
              void (async () => {
                const name = 'New Folder';
                const nodeId = await foldersController.create(name);
                if (!nodeId) return;
                setPendingSidebarRenameNodeId(nodeId);
                const folderId = parseInt(nodeId.replace('folder:', ''), 10);
                if (isNaN(folderId)) return;
                await entityMutations.updateTargetFolderMembership(effectiveTarget, folderId, 'add');
                entityMutations.settleSelectionAfterMutation();
              })();
            } : undefined,
            onSearchByImage: (engine, hash) => {
              void reverseImageSearch(hash, engine).catch((reason) => showErrorNotification({
                title: t("Reverse image search failed"),
                message: reason instanceof Error ? reason.message : String(reason),
              }));
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
            onExportPictoPack: () => {
              if (!effectiveTarget) return;
              filesController.requestPictoPackExport(
                { kind: 'items', target: effectiveTarget },
                selCount,
              );
            },
            onExportOriginals: () => {
              if (!effectiveTarget) return;
              void filesController.chooseAndExportOriginals(effectiveTarget).catch((reason) => showErrorNotification({
                title: t("Could not export originals"),
                message: reason instanceof Error ? reason.message : String(reason),
              }));
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
          const menuId = contextMenu.openAt(pos, entries);
          if (openWithPending && singleItem?.content_hash) {
            const hash = singleItem.content_hash;
            void filesController.getOpenWithOptionsForHash(hash)
              .then((resolvedOptions) => {
                const replacement = buildEntityOpenContextEntries({
                  hash,
                  openWithOptions: resolvedOptions,
                  onOpenWithApplication,
                  onOpenWithChooser,
                }).filter((entry) => 'label' in entry && entry.label.startsWith('Open With Other'));
                contextMenu.replaceEntry(menuId, 'Open With Other', replacement);
              })
              .catch((reason) => {
                console.warn('[grid] associated application lookup failed', reason);
                contextMenu.replaceEntry(menuId, 'Open With Other', []);
              });
          }
        }}
        onEmptyContextMenu={(pos) => { void openEmptyGridContextMenu(pos); }}
        onLoadMore={cursor ? () => gridController.loadNextPage() : undefined}
      />
    );
  };

  return (
    <div className={styles.root}>
      {!viewerSession ? <GridFilterToolbar /> : null}
      <div className={styles.surfaceViewport}>
        {renderIncomingSurface()}
      </div>
      {viewerSession && viewerItem?.kind === 'collection' ? (
        <GroupSurface
          key={`${viewerItem.root_id}:${groupInitialMode}`}
          groupId={viewerItem.root_id}
          initialMode={groupInitialMode}
          breadcrumbParent={collectionBreadcrumbParent}
          rootCurrentIndex={viewerIndex}
          rootTotal={totalCount ?? items.length}
          onNavigateRoot={navigateRootDetail}
          onClose={() => closeRootDetail(viewerItem.root_id)}
          recordItemId={gridScope.kind === 'recently_viewed' ? null : viewerItem.root_id}
        />
      ) : null}

      {!viewerSession && !quickLookSession && fileDragOver && (
        <div className={styles.dropOverlay}>
          <div className={styles.dropOverlayBadge}>
            {t("Drop files to import")}{gridScope.kind === 'folder' && <span className={styles.dropOverlaySub}>{t("into current folder")}</span>}
          </div>
        </div>
      )}

      {viewerSession && viewerItem?.kind !== 'collection' && (
        <MediaView
          items={items}
          currentIndex={viewerIndex}
          totalCount={totalCount}
          recordItemId={gridScope.kind === 'recently_viewed' ? null : undefined}
          onNavigate={navigateRootDetail}
          onClose={closeRootDetail}
          onLoadMore={cursor ? () => gridController.loadNextPage() : undefined}
        />
      )}

      {quickLookSession ? (
        <GridQuickLook
          items={items}
          currentIndex={quickLookIndex}
          totalCount={totalCount}
          recordItemId={gridScope.kind === 'recently_viewed' ? null : undefined}
          onNavigate={navigateQuickLook}
          onClose={closeQuickLook}
          onLoadMore={cursor ? () => gridController.loadNextPage() : undefined}
        />
      ) : null}

      {inboxReviewItemId != null && (viewerSession || quickLookSession) ? (
        <InboxReviewControls
          itemId={inboxReviewItemId}
          onCommit={commitInboxReview}
          onAdvance={advanceAfterInboxReview}
        />
      ) : null}

      {contextMenu.state && (
        <ContextMenu
          entries={contextMenu.state.entries}
          position={contextMenu.state.position}
          onClose={contextMenu.close}
        />
      )}

    </div>
  );
}
