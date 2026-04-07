/**
 * Grid screen — feature root. Reads state, delegates to CanvasGrid.
 */

import { useEffect, useRef, useState, useCallback } from 'react';
import { useAtomValue, useSetAtom, getDefaultStore } from 'jotai';
import { IconPhoto, IconUpload, IconFolderPlus } from '@tabler/icons-react';
import * as entityMutations from '../../controllers/entityMutations';
import { getShortcut, matchesShortcutDef } from '../../shared/lib/shortcuts';
import { activeNodeIdAtom, parentNodeIdAtom, collectionNameAtom, skipFadeOutAtom } from '../../state/navigation';
import {
  gridItemsAtom,
  gridLoadingAtom,
  gridErrorAtom,
  gridCursorAtom,
  gridViewModeAtom,
  gridTargetSizeAtom,
  gridShowNameAtom,
  gridShowExtensionAtom,
  gridShowResolutionAtom,
  gridFitThumbnailsAtom,
  gridSearchTextAtom,
  gridTotalCountAtom,
  gridTotalSizeBytesAtom,
  gridScopeAtom,
  gridTransitionPhaseAtom,
  gridSoftTransitionActionAtom,
  gridShowSubfoldersAtom,
  gridChildFoldersAtom,
  activeGridScopeAtom,
} from '../../state/grid';
import { gridController } from '../../controllers/gridController';
import { foldersController } from '../../controllers/foldersController';
import { isNativeDragPending, isDragActive as isDragActiveCheck, isInternalDragOrigin } from './dragState';
import {
  clearSelectionAtom,
  querySelectionActiveAtom,
  selectAllResultsAtom,
  selectedEntityHashesAtom,
  selectedSubfolderNodeIdsAtom,
  selectionCountAtom,
  selectionModeAtom,
  selectionTargetAtom,
  toggleQuerySelectionHashAtom,
} from '../../state/selection';
import {
  displayedGridSnapshotAtom,
  displayedInspectorTargetAtom,
  displayedInspectorEntityDataAtom,
  inspectorLoadingAtom,
  inspectorErrorAtom,
  liveInspectorTargetAtom,
  subfolderPreviewAtom,
} from '../../state/inspector';
import { sidebarNodesAtom } from '../../state/sidebar';
import { CanvasGrid } from './canvas/CanvasGrid';
import { SubfolderGrid } from './SubfolderGrid';
import { ContextMenu, useContextMenu } from '../../shared/ui/ContextMenu';
import { buildTileContextMenu, buildEmptyContextMenu } from './gridContextMenu';
import { saveScrollPosition, getScrollPosition, pushHistory } from '../../state/navigationHistory';
import { viewerSessionAtom, quickLookSessionAtom, createViewerSession, navigateViewerSession } from '../../state/viewer';
import { tagSelectOpenAtom, folderPickerOpenAtom, aiTaggerOpenAtom, batchRenameOpenAtom } from '../../state/portals';
import { confirmModalAtom, folderImportModalAtom, exportModalAtom } from '../../state/modals';
import { MediaView } from '../viewer/MediaView';
import { SubscriptionsScreen } from '../subscriptions/SubscriptionsScreen';
import { QuickLook } from '../viewer/QuickLook';
import { TagSelectPanel } from '../tags/TagSelectPanel';
import { FolderPickerPanel } from '../folders/FolderPickerPanel';
import { AiTaggerPanel } from '../ai-tagger/AiTaggerPanel';
import { BatchRenamePanel } from '../batch-rename/BatchRenamePanel';
import { useGridArrowNav } from './hooks/useGridArrowNav';
import type { LayoutResult } from './layout/types';
import { windowController } from '../../controllers/windowController';
import { filesController } from '../../controllers/filesController';
import { collectionsController } from '../../controllers/collectionsController';
import { viewerController } from '../../controllers/viewerController';
import { nodeIdToGridScope } from '../../shared/lib/gridScope';
import styles from './GridScreen.module.css';

// ── Smart collection naming (ported from legacy) ──

const GENERATED_NAME_RE = /^(?:[a-f0-9]{24,}|image[_-]?\d+|img[_-]?\d+|file[_-]?\d+)$/i;

function normalizeNameBase(name: string): string {
  return name.trim()
    .replace(/\.[a-z0-9]{2,5}$/i, '')
    .replace(/(?:[\s._-]|\s*\(\s*)\d+\s*\)?$/g, '')
    .trim()
    .toLowerCase();
}

function inferCollectionName(memberNames: string[]): string {
  const now = new Date();
  const fallback = `Collection ${now.toLocaleDateString()} ${now.toLocaleTimeString()}`;
  const names = memberNames.map((n) => n.trim()).filter(Boolean);
  if (names.length === 0) return fallback;
  const allGenerated = names.every((n) => GENERATED_NAME_RE.test(n));
  const bases = names.map(normalizeNameBase).filter(Boolean);
  const uniqueBases = new Set(bases);
  if (uniqueBases.size === 1 && bases.length > 0) {
    return names.find((n) => normalizeNameBase(n) === bases[0]) ?? fallback;
  }
  if (!allGenerated) return names[0];
  return fallback;
}

const store = getDefaultStore();
const SCOPE_TRANSITION_MS = 170;
const STATUS_ACTIVE = 1;
const STATUS_TRASH = 2;

export function GridScreen() {
  const activeNodeId = useAtomValue(activeNodeIdAtom);
  const setActiveNodeId = useSetAtom(activeNodeIdAtom);
  const parentNodeId = useAtomValue(parentNodeIdAtom);
  const setParentNodeId = useSetAtom(parentNodeIdAtom);
  const setCollectionName = useSetAtom(collectionNameAtom);
  const items = useAtomValue(gridItemsAtom);
  const loading = useAtomValue(gridLoadingAtom);
  const error = useAtomValue(gridErrorAtom);
  const cursor = useAtomValue(gridCursorAtom);
  const viewMode = useAtomValue(gridViewModeAtom);
  const targetSize = useAtomValue(gridTargetSizeAtom);
  const showName = useAtomValue(gridShowNameAtom);
  const showExtension = useAtomValue(gridShowExtensionAtom);
  const showResolution = useAtomValue(gridShowResolutionAtom);
  const fitThumbnails = useAtomValue(gridFitThumbnailsAtom);
  const softTransitionAction = useAtomValue(gridSoftTransitionActionAtom);
  const setSoftTransitionAction = useSetAtom(gridSoftTransitionActionAtom);
  const searchText = useAtomValue(gridSearchTextAtom);
  const totalCount = useAtomValue(gridTotalCountAtom);
  const totalSizeBytes = useAtomValue(gridTotalSizeBytesAtom);
  const gridScope = useAtomValue(gridScopeAtom);
  const activeGridScope = useAtomValue(activeGridScopeAtom);
  const sidebarNodes = useAtomValue(sidebarNodesAtom);
  const selectedHashes = useAtomValue(selectedEntityHashesAtom);
  const selectedSubfolderNodeIds = useAtomValue(selectedSubfolderNodeIdsAtom);
  const selectionMode = useAtomValue(selectionModeAtom);
  const querySelectionActive = useAtomValue(querySelectionActiveAtom);
  const selectionCount = useAtomValue(selectionCountAtom);
  const selectionTarget = useAtomValue(selectionTargetAtom);
  const setSelectedHashes = useSetAtom(selectedEntityHashesAtom);
  const setSelectedSubfolderNodeIds = useSetAtom(selectedSubfolderNodeIdsAtom);
  const clearSelection = useSetAtom(clearSelectionAtom);
  const selectAllResults = useSetAtom(selectAllResultsAtom);
  const toggleQuerySelectionHash = useSetAtom(toggleQuerySelectionHashAtom);
  const lastClickedIndexRef = useRef<number | null>(null);
  const viewerSession = useAtomValue(viewerSessionAtom);
  const setViewerSession = useSetAtom(viewerSessionAtom);
  const quickLookSession = useAtomValue(quickLookSessionAtom);
  const setQuickLookSession = useSetAtom(quickLookSessionAtom);
  const setTagSelectOpen = useSetAtom(tagSelectOpenAtom);
  const setFolderPickerOpen = useSetAtom(folderPickerOpenAtom);
  const setAiTaggerOpen = useSetAtom(aiTaggerOpenAtom);
  const setBatchRenameOpen = useSetAtom(batchRenameOpenAtom);
  const gridContainerRef = useRef<HTMLDivElement | null>(null);
  const gridLayoutRef = useRef<LayoutResult | null>(null);
  const [renamingIndex, setRenamingIndex] = useState<number | null>(null);

  const scrollToItem = useCallback((index: number) => {
    const layout = gridLayoutRef.current;
    const container = gridContainerRef.current;
    if (!layout || !container || index < 0 || index >= layout.positions.length) return;
    const pos = layout.positions[index];
    if (!pos) return;
    const scrollTop = container.scrollTop;
    const viewportH = container.clientHeight;
    if (pos.y < scrollTop + 16) {
      container.scrollTop = pos.y - 16;
    } else if (pos.y + pos.h > scrollTop + viewportH - 16) {
      container.scrollTop = pos.y + pos.h - viewportH + 16;
    }
  }, []);

  useGridArrowNav({
    items,
    layoutRef: gridLayoutRef,
    containerRef: gridContainerRef,
    selectedHashes,
    setSelectedHashes,
    lastClickedIndexRef,
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
      const folderId = scope.kind === 'folder' ? scope.id : null;

      // Detect folder drop (single path without media extension)
      const mediaExt = /\.(jpe?g|png|gif|webp|bmp|tiff?|svg|mp4|mkv|webm|avi|mov|wmv|flv|m4v|avif|jxl|ico|pdf)$/i;
      if (paths.length === 1 && !mediaExt.test(paths[0])) {
        // Show import modal for folder drops
        store.set(folderImportModalAtom, {
          open: true,
          path: paths[0],
          preserveStructure: true,
          targetFolderId: folderId ?? null,
        });
      } else {
        // File import — direct
        void filesController.importFiles(paths, folderId != null ? { parent_folder_id: folderId } : {});
      }
    });

    return () => { promise.then((fn: () => void) => fn()); };
  }, []);

  const showSubfolders = useAtomValue(gridShowSubfoldersAtom);
  const childFolders = useAtomValue(gridChildFoldersAtom);
  const setSubfolderPreview = useSetAtom(subfolderPreviewAtom);
  const contextMenu = useContextMenu();

  // Fetch preview data when a subfolder tile is selected
  useEffect(() => {
    // Find the single selected folder hash
    const hashes = [...selectedSubfolderNodeIds];
    const folderHash = hashes.length === 1 ? hashes[0] : null;
    if (!folderHash) {
      setSubfolderPreview(null);
      return;
    }
    const folderId = parseInt(folderHash.replace('folder:', ''), 10);
    if (isNaN(folderId)) return;
    let cancelled = false;
    void gridController.loadSubfolderPreview(folderId, 4).then((page) => {
      if (cancelled) return;
      setSubfolderPreview({
        nodeId: folderHash,
        items: page.items.slice(0, 4),
        totalCount: page.total_count,
        totalSizeBytes: page.total_size_bytes ?? null,
      });
    }).catch(() => {});
    return () => { cancelled = true; };
  }, [selectedSubfolderNodeIds, setSubfolderPreview]);

  // Reset range anchor when items change (scope nav, sort, search, reload)
  useEffect(() => { lastClickedIndexRef.current = null; }, [items]);
  const setDisplayedGridSnapshot = useSetAtom(displayedGridSnapshotAtom);
  const setDisplayedInspectorTarget = useSetAtom(displayedInspectorTargetAtom);
  const setDisplayedEntityData = useSetAtom(displayedInspectorEntityDataAtom);
  const setInspectorLoading = useSetAtom(inspectorLoadingAtom);
  const setInspectorError = useSetAtom(inspectorErrorAtom);
  const liveTarget = useAtomValue(liveInspectorTargetAtom);

  type TransitionPhase = 'idle' | 'fading_out' | 'waiting' | 'fading_in';
  const [transitionPhase, setTransitionPhaseRaw] = useState<TransitionPhase>('idle');
  const transitionPhaseRef = useRef<TransitionPhase>('idle');
  const setTransitionPhase = useCallback((phase: TransitionPhase | ((prev: TransitionPhase) => TransitionPhase)) => {
    const resolved = typeof phase === 'function' ? phase(transitionPhaseRef.current) : phase;
    if (resolved === transitionPhaseRef.current) return;
    transitionPhaseRef.current = resolved;
    // Synchronous atom update — gridSettle reads this immediately via store.get()
    store.set(gridTransitionPhaseAtom, resolved);
    // React state for component re-renders
    setTransitionPhaseRaw(resolved);
  }, []);
  const lastScrollTopRef = useRef(0);
  const previousNodeIdRef = useRef(activeNodeId);
  const transitionTimerRef = useRef<number | null>(null);
  const fadeInFrameRef = useRef<number | null>(null);
  const pendingNodeIdRef = useRef(activeNodeId);
  const itemsLengthRef = useRef(items.length);
  itemsLengthRef.current = items.length;
  /** Scroll position to restore for the incoming scope (set during transition). */
  const restoredScrollTopRef = useRef<number | null>(null);

  const scope = activeGridScope;
  const isGridScope = scope !== null;

  const clearTransition = useCallback(() => {
    if (transitionTimerRef.current != null) {
      window.clearTimeout(transitionTimerRef.current);
      transitionTimerRef.current = null;
    }
    if (fadeInFrameRef.current != null) {
      window.cancelAnimationFrame(fadeInFrameRef.current);
      fadeInFrameRef.current = null;
    }
    setTransitionPhase('idle');
  }, []);

  const beginFadeIn = useCallback(() => {
    if (fadeInFrameRef.current != null) {
      window.cancelAnimationFrame(fadeInFrameRef.current);
      fadeInFrameRef.current = null;
    }
    fadeInFrameRef.current = window.requestAnimationFrame(() => {
      fadeInFrameRef.current = null;
      setTransitionPhase((phase) => {
        if (phase !== 'waiting') return phase;
        if (transitionTimerRef.current != null) {
          window.clearTimeout(transitionTimerRef.current);
        }
        transitionTimerRef.current = window.setTimeout(() => {
          transitionTimerRef.current = null;
          setTransitionPhase('idle');
        }, SCOPE_TRANSITION_MS);
        return 'fading_in';
      });
    });
  }, []);

  useEffect(() => {
    const previousScope = nodeIdToGridScope(previousNodeIdRef.current);
    const nextScope = activeGridScope;
    pendingNodeIdRef.current = activeNodeId;

    if (transitionTimerRef.current != null) {
      window.clearTimeout(transitionTimerRef.current);
      transitionTimerRef.current = null;
    }
    if (fadeInFrameRef.current != null) {
      window.cancelAnimationFrame(fadeInFrameRef.current);
      fadeInFrameRef.current = null;
    }

    if (!nextScope) {
      // Navigating to non-grid (subscriptions, etc.) — no fade, just deactivate immediately
      if (previousScope) saveScrollPosition(previousNodeIdRef.current, lastScrollTopRef.current);
      gridController.deactivate();
      previousNodeIdRef.current = '';
      clearTransition();
      return;
    }

    if (previousScope) {
      saveScrollPosition(previousNodeIdRef.current, lastScrollTopRef.current);

      // Skip fade-out when requested (e.g. after manual fade-out for collection creation)
      const skip = store.get(skipFadeOutAtom);
      if (skip) {
        store.set(skipFadeOutAtom, false);
        restoredScrollTopRef.current = getScrollPosition(activeNodeId);
        setTransitionPhase('waiting');
        void gridController.navigateTo(nodeIdToGridScope(activeNodeId)!);
        previousNodeIdRef.current = activeNodeId;
        return;
      }

      // Grid-to-grid: fade out old → wait → load new → fade in
      setTransitionPhase('fading_out');
      transitionTimerRef.current = window.setTimeout(() => {
        transitionTimerRef.current = null;
        const committedNodeId = pendingNodeIdRef.current;
        restoredScrollTopRef.current = getScrollPosition(committedNodeId);
        setTransitionPhase('waiting');
        void gridController.navigateTo(nodeIdToGridScope(committedNodeId)!);
        previousNodeIdRef.current = committedNodeId;
      }, SCOPE_TRANSITION_MS);
      return;
    }

    // Non-grid to grid: no fade-out, start in waiting for clean fade-in
    setTransitionPhase('waiting');
    restoredScrollTopRef.current = getScrollPosition(activeNodeId);
    void gridController.navigateTo(nextScope);
    previousNodeIdRef.current = activeNodeId;
  }, [activeGridScope, activeNodeId, clearTransition]);

  useEffect(() => {
    if (transitionPhase !== 'waiting') return;
    if (!loading) {
      beginFadeIn();
    }
  }, [beginFadeIn, loading, transitionPhase]);

  // Soft transition: sort/layout change within same scope.
  // Fade out → execute deferred action at midpoint → fade in.
  useEffect(() => {
    if (!softTransitionAction) return;
    if (transitionPhase !== 'idle') {
      // Already transitioning — execute immediately, skip fade
      softTransitionAction();
      setSoftTransitionAction(null);
      return;
    }

    setTransitionPhase('fading_out');
    const action = softTransitionAction;
    setSoftTransitionAction(null);

    transitionTimerRef.current = window.setTimeout(() => {
      transitionTimerRef.current = null;
      // Execute the deferred action (sort change, layout change, etc.)
      action();
      setTransitionPhase('waiting');
      // waiting→fading_in effect will fire once loading completes
    }, SCOPE_TRANSITION_MS);
  }, [softTransitionAction, transitionPhase, setTransitionPhase, setSoftTransitionAction]);

  useEffect(() => () => {
    if (transitionTimerRef.current != null) {
      window.clearTimeout(transitionTimerRef.current);
    }
    if (fadeInFrameRef.current != null) {
      window.cancelAnimationFrame(fadeInFrameRef.current);
    }
  }, []);

  // Commit the displayed scene — snapshot + inspector target — atomically.
  // ONLY commits during fading_in (new data arriving after transition).
  // During idle: only commits if data changed within the SAME scope (reconcile, sort, search).
  const displayedNodeIdRef = useRef(activeNodeId);

  useEffect(() => {
    if (!isGridScope) {
      if (transitionPhase === 'idle' || transitionPhase === 'fading_in') {
        displayedNodeIdRef.current = activeNodeId;
        setDisplayedGridSnapshot(null);
        setDisplayedInspectorTarget({ kind: 'none' });
        setDisplayedEntityData(null);
        setInspectorLoading(false);
        setInspectorError(null);
      }
      return;
    }

    // During fading_in: commit only when data is loaded (not loading)
    // During idle: only commit if we're on the SAME scope (data update, not scope change)
    const isSameScope = activeNodeId === displayedNodeIdRef.current;
    const shouldCommit = (transitionPhase === 'fading_in' && !loading) || (transitionPhase === 'idle' && isSameScope);

    if (shouldCommit) {
      displayedNodeIdRef.current = activeNodeId;
      setDisplayedGridSnapshot({
        nodeId: activeNodeId,
        previewItems: items.slice(0, 4),
        totalCount,
        totalSizeBytes,
        searchText: searchText.trim(),
        sidebarNode: sidebarNodes.find((n) => n.id === activeNodeId) ?? null,
      });
      // Don't overwrite inspector target when a subfolder tile is selected
      // (liveTarget points to the subfolder, not the current scope)
      const isSubfolderSelected = liveTarget.kind === 'scope' && 'nodeId' in liveTarget && liveTarget.nodeId !== activeNodeId;
      if (!isSubfolderSelected && (transitionPhase === 'fading_in' || liveTarget.kind === 'scope' || liveTarget.kind === 'none')) {
        setDisplayedInspectorTarget(
          liveTarget.kind === 'none'
            ? { kind: 'none' }
            : { kind: 'scope', nodeId: activeNodeId },
        );
        setDisplayedEntityData(null);
        setInspectorLoading(false);
        setInspectorError(null);
      }
    }
  }, [
    activeNodeId,
    isGridScope,
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
    setFolderPickerOpen(true);
  }, [selectionTarget, setFolderPickerOpen]);

  const removeSelectionFromCurrentFolder = useCallback(async () => {
    if (!selectionTarget || gridScope.kind !== 'folder' || gridScope.id == null) return;
    await entityMutations.updateTargetFolderMembership(selectionTarget, gridScope.id, 'remove');
  }, [gridScope, selectionTarget]);

  const setSelectionStatus = useCallback(async (status: number) => {
    if (!selectionTarget) return;
    await entityMutations.setTargetStatus(selectionTarget, status);
  }, [selectionTarget]);

  const permanentlyDeleteSelection = useCallback(() => {
    if (!selectionTarget) return;
    store.set(confirmModalAtom, {
      open: true,
      title: 'Delete Permanently',
      message: `This will permanently delete ${selectionCount} item${selectionCount !== 1 ? 's' : ''}. This cannot be undone.`,
      confirmLabel: 'Delete',
      danger: true,
      onConfirm: () => {
        void entityMutations.permanentlyDeleteTarget(selectionTarget);
        clearSelection();
      },
    });
  }, [selectionTarget, selectionCount, clearSelection]);

  // ── Detail window communication ──
  // When a detail window opens, it sends 'detail-window-ready' with { hash }.
  // We respond with ONLY the selected images (not the entire grid).
  // Single selection → one image, no navigation in detail window.
  // Multi selection → those images as a navigable set.
  const detailWindowSelectionRef = useRef(new Map<string, string[]>());
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
      const selectedHashes = detailWindowSelectionRef.current.get(label);
      if (!selectedHashes) return;
      const curItems = itemsRef.current;
      const lightImages = selectedHashes
        .map((h: string) => curItems.find((i: any) => i.entity_hash === h))
        .filter(Boolean)
        .map((i: any) => ({
          hash: i.entity_hash,
          name: i.name,
          mime: i.mime_type,
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
  const selectedHashesRef = useRef(selectedHashes);
  selectedHashesRef.current = selectedHashes;
  const itemsRef = useRef(items);
  itemsRef.current = items;
  const viewerSessionRef = useRef(viewerSession);
  viewerSessionRef.current = viewerSession;
  const quickLookSessionRef = useRef(quickLookSession);
  quickLookSessionRef.current = quickLookSession;
  const gridScopeRef = useRef(gridScope);
  gridScopeRef.current = gridScope;

  // Refs for setters used in the keydown handler (avoid re-registering on every render)
  const setTagSelectOpenRef = useRef(setTagSelectOpen);
  setTagSelectOpenRef.current = setTagSelectOpen;
  const setFolderPickerOpenRef = useRef(setFolderPickerOpen);
  setFolderPickerOpenRef.current = setFolderPickerOpen;

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
      const hashes = selectedHashesRef.current;
      const curItems = itemsRef.current;
      const scope = gridScopeRef.current;
      const isTrash = scope.kind === 'system' && scope.key === 'trash';
      const singleHash = count === 1 ? [...hashes][0] : null;

      if (matchesShortcutDef(e, defs.selectAll)) { e.preventDefault(); selectAllResults(); return; }
      if (matchesShortcutDef(e, defs.deselectAll) && count > 0) { clearSelection(); return; }

      if (matchesShortcutDef(e, defs.detailView) && singleHash && !viewerSessionRef.current && !quickLookSessionRef.current) {
        e.preventDefault(); setViewerSession(createViewerSession(curItems, singleHash)); return;
      }
      if (matchesShortcutDef(e, defs.quicklook) && !viewerSessionRef.current) {
        e.preventDefault();
        if (quickLookSessionRef.current) setQuickLookSession(null);
        else if (singleHash) setQuickLookSession(createViewerSession(curItems, singleHash));
        return;
      }

      if (matchesShortcutDef(e, defs.openDefault) && singleHash) {
        e.preventDefault(); void filesController.openDefaultAppForHash(singleHash); return;
      }
      if (matchesShortcutDef(e, defs.revealInFolder) && singleHash) {
        e.preventDefault(); void filesController.revealHashInFolder(singleHash); return;
      }
      if (matchesShortcutDef(e, defs.openNewWindow) && count > 0) {
        e.preventDefault();
        // Use first selected hash as the window identity
        const selectedArr = [...hashes];
        const primaryHash = singleHash ?? selectedArr[0];
        const item = curItems.find((i) => i.entity_hash === primaryHash);
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
        else void setSelectionStatus(STATUS_TRASH);
        return;
      }
      // Mod+Shift+Backspace: context-dependent reverse action
      if (matchesShortcutDef(e, defs.restore) && count > 0) {
        e.preventDefault();
        if (isTrash) void setSelectionStatus(STATUS_ACTIVE);
        else if (scope.kind === 'folder') void removeSelectionFromCurrentFolder();
        return;
      }

      if (matchesShortcutDef(e, defs.addToFolder) && count > 0) {
        e.preventDefault(); void addSelectionToFolder(); return;
      }

      // T — open tag select, F — open folder picker
      if (matchesShortcutDef(e, defs.addTag) && count > 0) {
        e.preventDefault(); setTagSelectOpenRef.current(true); return;
      }
      if (matchesShortcutDef(e, defs.addToFolders) && count > 0) {
        e.preventDefault(); setFolderPickerOpenRef.current(true); return;
      }

      // Rating keys 0-5 (plain digits, no modifiers)
      if (!e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey && count > 0) {
        const digit = parseInt(e.key, 10);
        if (digit >= 0 && digit <= 5) {
          e.preventDefault();
          void entityMutations.setTargetRating(
            { kind: 'entity_hashes', entity_hashes: [...hashes] },
            digit,
          );
          return;
        }
      }
    }
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [clearSelection, selectAllResults, setViewerSession, setQuickLookSession, setSelectionStatus, permanentlyDeleteSelection, addSelectionToFolder, removeSelectionFromCurrentFolder]);

  const incomingHidden = transitionPhase === 'waiting';
  const incomingFadingOut = transitionPhase === 'fading_out';
  const incomingFadingIn = transitionPhase === 'fading_in';
  const isEmpty = items.length === 0 && !loading;

  const renderIncomingSurface = () => {
    if (!isGridScope) {
      if (activeNodeId === 'system:subscriptions') {
        return <SubscriptionsScreen />;
      }
      return <div className={styles.nonGridPlaceholder}>This view is not available yet</div>;
    }

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
      const scopeKey = gridScope.kind === 'system' ? gridScope.key : gridScope.kind;
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
        <div className={styles.empty}>
          {/* Glass picture frame indicator */}
          <div className={styles.emptyFrame}>
            <div className={styles.emptyFrameGlass}>
              <div className={styles.emptyFrameInner}>
                <IconPhoto size={28} stroke={1.2} style={{ color: 'var(--color-bg-app)', opacity: 1 }} />
              </div>
            </div>
          </div>
          <span className={styles.emptyTitle}>{emptyTitle}</span>
          <span className={styles.emptyDesc}>{emptyDesc}</span>
          {showImport && (
            <div className={styles.emptyActions}>
              <button className={styles.emptyBtn} type="button" onClick={() => {
                void (async () => {
                  try {
                    const result = await (window as any).picto.dialog.open({
                      properties: ['openFile'], multiple: true, title: 'Import files',
                      filters: [{ name: 'Media', extensions: ['png','jpg','jpeg','gif','webp','bmp','mp4','webm','mkv','mov','avi'] }],
                    });
                    if (result) {
                      const paths = Array.isArray(result) ? result : [result];
                      await filesController.importFiles(paths);
                    }
                  } catch (err) {
                    console.error('[grid] import files failed:', err);
                  }
                })();
              }}>
                <IconUpload size={14} stroke={1.5} />
                Import Files
              </button>
              <button className={styles.emptyBtn} type="button" onClick={() => {
                void (async () => {
                  try {
                    const result = await (window as any).picto.dialog.open({
                      properties: ['openDirectory'], multiple: false, title: 'Import folder',
                    });
                    if (result) {
                      const folderPath = typeof result === 'string' ? result : result[0];
                      await filesController.importFolder(folderPath);
                    }
                  } catch (err) {
                    console.error('[grid] import folder failed:', err);
                  }
                })();
              }}>
                <IconFolderPlus size={14} stroke={1.5} />
                Import Folder
              </button>
            </div>
          )}
        </div>
      );
    }

    const hasSubfolders = showSubfolders && childFolders.length > 0 && gridScope.kind === 'folder';

    const subfolderHeader = hasSubfolders ? (
      <SubfolderGrid
        childFolders={childFolders}
        targetSize={targetSize}
        totalImageCount={items.length}
        selectedNodeIds={selectedSubfolderNodeIds}
        onOpenFolder={(nodeId) => {
          setParentNodeId(activeNodeId);
          pushHistory(nodeId);
          setActiveNodeId(nodeId);
        }}
        onSelectFolder={(nodeId, event) => {
          if (event.metaKey || event.ctrlKey) {
            setSelectedSubfolderNodeIds((prev) => {
              const next = new Set(prev);
              if (next.has(nodeId)) next.delete(nodeId); else next.add(nodeId);
              return next;
            });
          } else {
            setSelectedSubfolderNodeIds(new Set([nodeId]));
          }
        }}
        onFolderContextMenu={(nodeId, _folder, pos) => {
          // Select the folder if not already selected
          if (!selectedSubfolderNodeIds.has(nodeId)) {
            setSelectedSubfolderNodeIds(new Set([nodeId]));
          }
          const folderId = parseInt(nodeId.replace('folder:', ''), 10);
          if (isNaN(folderId)) return;
          const entries = buildTileContextMenu({
            selectionCount: 1,
            querySelectionActive: false,
            singleSelected: true,
            singleHash: nodeId,
            singleKind: 'folder',
            hasCollections: false,
            hasFolders: true,
            isMixed: false,
            isFoldersOnly: true,
            scopeKind: 'folder',
            statusFilter: null,
            loadedCount: items.length,
            onSelectAll: () => selectAllResults(),
            onDeselectAll: () => clearSelection(),
            onOpen: () => {
              setParentNodeId(activeNodeId);
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
        showResolution={showResolution}
        fitThumbnails={fitThumbnails}
        totalCount={totalCount}
        suppressTileReveal={transitionPhase === 'fading_out' || transitionPhase === 'waiting'}
        selectedEntityHashes={selectedHashes}
        initialScrollTop={restoredScrollTopRef.current}
        onContainerRef={(el) => { gridContainerRef.current = el; }}
        onLayoutChange={(l) => { gridLayoutRef.current = l; }}
        renamingIndex={renamingIndex}
        onRenameCommit={(idx, name) => {
          setRenamingIndex(null);
          const item = items[idx];
          if (item && name) void entityMutations.setEntityName(item.entity_hash, name);
        }}
        onRenameCancel={() => setRenamingIndex(null)}
        onFirstPaint={() => { restoredScrollTopRef.current = null; beginFadeIn(); }}
        onScrollTopChange={(scrollTop) => { lastScrollTopRef.current = scrollTop; }}
        onTileClick={(index, item, event) => {
          const hash = item.entity_hash;
          if (event?.shiftKey && lastClickedIndexRef.current != null) {
            const from = Math.min(lastClickedIndexRef.current, index);
            const to = Math.max(lastClickedIndexRef.current, index);
            const base = (event.metaKey || event.ctrlKey)
              ? new Set(selectedHashes)
              : new Set<string>();
            for (let i = from; i <= to; i++) {
              if (items[i]) base.add(items[i].entity_hash);
            }
            setSelectedHashes(base);
          } else if (event?.metaKey || event?.ctrlKey) {
            if (selectionMode === 'query_results') {
              toggleQuerySelectionHash(hash);
            } else {
              setSelectedHashes((prev) => {
                const next = new Set(prev);
                if (next.has(hash)) next.delete(hash);
                else next.add(hash);
                return next;
              });
            }
            lastClickedIndexRef.current = index;
          } else {
            setSelectedHashes(new Set([hash]));
            lastClickedIndexRef.current = index;
          }
        }}
        onTileDoubleClick={(_index, item) => {
          // Both singles and collections open in MediaView (collections show StripView)
          setViewerSession(createViewerSession(items, item.entity_hash));
        }}
        onEmptyClick={() => clearSelection()}
        onSelectionChange={setSelectedHashes}
        onTileContextMenu={(index, item, pos) => {
          // Ensure the right-clicked tile is selected
          let effectiveHashes = selectedHashes;
          let effectiveSelectionMode = selectionMode;
          let effectiveSelectionCount = selectionCount;
          let effectiveQuerySelectionActive = querySelectionActive;
          if (!selectedHashes.has(item.entity_hash)) {
            effectiveHashes = new Set([item.entity_hash]);
            setSelectedHashes(effectiveHashes);
            lastClickedIndexRef.current = index;
            effectiveSelectionMode = 'explicit';
            effectiveSelectionCount = 1;
            effectiveQuerySelectionActive = false;
          }

          // Derive context for menu builder
          const selCount = effectiveSelectionCount;
          const selectedItems = items.filter((it) => effectiveHashes.has(it.entity_hash));
          const singleItem = effectiveSelectionMode === 'explicit' && selCount === 1 ? selectedItems[0] : null;
          const scopeKind = gridScope.kind === 'system' ? 'system'
            : gridScope.kind === 'folder' ? 'folder'
            : gridScope.kind === 'smart_folder' ? 'smart_folder'
            : gridScope.kind === 'collection' ? 'collection'
            : null;
          const statusFilter = gridScope.kind === 'system'
            ? (gridScope.key === 'inbox' ? 'inbox' : gridScope.key === 'trash' ? 'trash' : gridScope.key === 'all' ? 'active' : null)
            : null;

          const entries = buildTileContextMenu({
            selectionCount: selCount,
            querySelectionActive: effectiveQuerySelectionActive,
            singleSelected: effectiveSelectionMode === 'explicit' && selCount === 1,
            singleHash: singleItem?.entity_hash ?? null,
            singleKind: singleItem?.entity_kind ?? null,
            hasCollections: selectedItems.some((it) => it.entity_kind === 'collection'),
            scopeKind,
            collectionId: gridScope.kind === 'collection' ? gridScope.id : null,
            statusFilter,
            loadedCount: items.length,
            onSelectAll: () => selectAllResults(),
            onDeselectAll: () => clearSelection(),
            onOpen: singleItem ? () => setViewerSession(createViewerSession(items, singleItem.entity_hash)) : undefined,
            onOpenNewWindow: (hash) => {
              const it = items.find((i) => i.entity_hash === hash);
              const selectedArr = [...effectiveHashes];
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
            singleMime: singleItem?.mime_type ?? null,
            onCopyLink: (hash, mime) => {
              const ext: Record<string, string> = { 'image/jpeg': 'jpg', 'image/png': 'png', 'image/gif': 'gif', 'image/webp': 'webp', 'video/mp4': 'mp4', 'video/webm': 'webm' };
              filesController.copyText(`media://localhost/file/${hash}.${ext[mime] ?? 'bin'}`);
            },
            onRename: singleItem ? () => {
              const idx = items.findIndex((i) => i.entity_hash === singleItem.entity_hash);
              if (idx >= 0) setRenamingIndex(idx);
            } : undefined,
            onRegenerateThumbnails: () => {
              const hashes = [...effectiveHashes];
              void filesController.regenerateThumbnailsBatch(hashes);
            },
            onCopyTags: () => {
              if (!singleItem) return;
              void viewerController.getEntityDetails(singleItem.entity_hash).then((d) => {
                if (!d?.tags) return;
                const tagStrings = d.tags.map((t) =>
                  t.namespace && t.namespace !== 'default' ? `${t.namespace}:${t.subtag}` : t.subtag,
                );
                filesController.copyText(JSON.stringify(tagStrings));
                (window as any).__pictoClipboardTags = tagStrings;
              });
            },
            onPasteTags: () => {
              const tags = (window as any).__pictoClipboardTags as string[] | undefined;
              if (!tags?.length) return;
              void entityMutations.addTargetTags(selectionTarget!, tags);
            },
            hasClipboardTags: !!((window as any).__pictoClipboardTags as string[] | undefined)?.length,
            onAddToFolder: () => { setFolderPickerOpen(true); },
            onNewFolderWithSelection: selectionTarget ? () => {
              void (async () => {
                const name = 'New Folder';
                const nodeId = await foldersController.create(name);
                if (!nodeId) return;
                const folderId = parseInt(nodeId.replace('folder:', ''), 10);
                if (isNaN(folderId)) return;
                for (const hash of effectiveHashes) {
                  void entityMutations.updateTargetFolderMembership(
                    { kind: 'entity_hashes', entity_hashes: [hash] }, folderId, 'add',
                  );
                }
              })();
            } : undefined,
            onMergeIntoCollection: (() => {
              // Show "Merge into Collection" when selection has exactly 1 collection + other items
              if (selCount < 2) return undefined;
              const collections = selectedItems.filter((i) => i.entity_kind === 'collection');
              const nonCollections = selectedItems.filter((i) => i.entity_kind !== 'collection');
              if (collections.length !== 1 || nonCollections.length === 0) return undefined;
              const collId = collections[0].entity_id;
              return () => {
                void (async () => {
                  await collectionsController.addMembers(collId, nonCollections.map((i) => i.entity_hash));
                  // Merged items are now inside the collection — remove from grid, refresh collection tile
                  gridController.removeItems(nonCollections.map((i) => i.entity_hash));
                  void gridController.reconcile(false);
                })();
              };
            })(),
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
              void entityMutations.setTargetRating(
                { kind: 'entity_hashes', entity_hashes: [...effectiveHashes] },
                rating,
              );
            },
            onExport: () => {
              store.set(exportModalAtom, {
                open: true,
                fileCount: effectiveHashes.size,
                target: { kind: 'entity_hashes', entity_hashes: [...effectiveHashes] },
              });
            },
            onRemoveFromFolder: () => { void removeSelectionFromCurrentFolder(); },
            onOpenTagSelect: () => { setTagSelectOpen(true); },
            onOpenAiTagger: () => { setAiTaggerOpen(true); },
            onOpenBatchRename: () => { setBatchRenameOpen(true); },
            onCreateCollection: () => {
              const hashes = [...effectiveHashes];
              const selectedItems = items.filter((i) => effectiveHashes.has(i.entity_hash));
              const name = inferCollectionName(selectedItems.map((i) => i.name ?? ''));
              setTransitionPhase('fading_out');
              // Do backend work + navigate after fade-out completes
              const backendWork = (async () => {
                const id = await collectionsController.create(name);
                await collectionsController.addMembers(id, hashes);
                return id;
              })();
              setTimeout(async () => {
                const id = await backendWork;
                const collNodeId = `collection:${id}`;
                setParentNodeId(activeNodeId);
                setCollectionName(name);
                pushHistory(collNodeId);
                store.set(skipFadeOutAtom, true);
                setActiveNodeId(collNodeId);
              }, SCOPE_TRANSITION_MS);
            },
            onRemoveFromCollection: gridScope.kind === 'collection' && gridScope.id != null
              ? () => { void collectionsController.removeMembers(gridScope.id!, [...effectiveHashes]); }
              : undefined,
            onEditCollection: singleItem?.entity_kind === 'collection' ? () => {
              const collNodeId = `collection:${singleItem.entity_id}`;
              setParentNodeId(activeNodeId);
              setCollectionName(singleItem.name);
              pushHistory(collNodeId);
              setActiveNodeId(collNodeId);
            } : undefined,
            onSplitCollection: (() => {
              // Inside collection view — split and navigate back instantly
              if (gridScope.kind === 'collection' && gridScope.id != null) {
                return () => {
                  void (async () => {
                    const memberHashes = await collectionsController.listMemberHashes(gridScope.id!);
                    await collectionsController.delete(gridScope.id!);
                    const target = parentNodeId ?? 'system:active';
                    setParentNodeId(null);
                    setCollectionName(null);
                    store.set(skipFadeOutAtom, true);
                    setActiveNodeId(target);
                    setTimeout(() => setSelectedHashes(new Set(memberHashes)), 100);
                  })();
                };
              }
              // Single collection tile selected in normal grid — split and select freed members
              if (singleItem?.entity_kind === 'collection') {
                return () => {
                  void (async () => {
                    const memberHashes = await collectionsController.listMemberHashes(singleItem.entity_id);
                    await collectionsController.delete(singleItem.entity_id);
                    setTimeout(() => setSelectedHashes(new Set(memberHashes)), 100);
                  })();
                };
              }
              return undefined;
            })(),
            onMoveToTrash: () => { void setSelectionStatus(STATUS_TRASH); },
            onRestore: () => { void setSelectionStatus(STATUS_ACTIVE); },
            onPermanentDelete: () => { void permanentlyDeleteSelection(); },
            onAccept: () => { void setSelectionStatus(STATUS_ACTIVE); },
            onReject: () => { void setSelectionStatus(STATUS_TRASH); },
          });
          contextMenu.openAt(pos, entries);
        }}
        onEmptyContextMenu={(pos) => {
          // Don't clear selection — let the menu reflect current state
          const entries = buildEmptyContextMenu({
            selectionCount,
            querySelectionActive,
            singleSelected: selectionCount === 1,
            singleHash: selectionCount === 1 ? [...selectedHashes][0] ?? null : null,
            singleKind: null,
            hasCollections: false,
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
      <div
        className={`${styles.surface} ${
          incomingHidden
            ? styles.surfaceIncomingHidden
            : incomingFadingOut
              ? styles.surfaceFadeOut
              : incomingFadingIn
                ? styles.surfaceIncomingFadeIn
                : styles.surfaceIncomingVisible
        }`}
      >
        {renderIncomingSurface()}
      </div>

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
          currentIndex={viewerSession.currentIndex}
          totalCount={totalCount}
          onNavigate={(delta) => {
            const next = navigateViewerSession(viewerSession, items, delta);
            if (next) {
              setViewerSession(next);
              setSelectedHashes(new Set([next.currentHash]));
            }
          }}
          onClose={(exitHash) => {
            setViewerSession(null);
            if (exitHash) {
              setSelectedHashes(new Set([exitHash]));
              const idx = items.findIndex((i) => i.entity_hash === exitHash);
              if (idx >= 0) lastClickedIndexRef.current = idx;
              scrollToItem(idx);
            }
          }}
          onLoadMore={cursor ? () => gridController.loadNextPage() : undefined}
        />
      )}

      {quickLookSession && (
        <QuickLook
          items={items}
          currentIndex={quickLookSession.currentIndex}
          totalCount={totalCount}
          onNavigate={(delta) => {
            const next = navigateViewerSession(quickLookSession, items, delta);
            if (next) {
              setQuickLookSession(next);
              setSelectedHashes(new Set([next.currentHash]));
            }
          }}
          onClose={(exitHash) => {
            setQuickLookSession(null);
            if (exitHash) {
              setSelectedHashes(new Set([exitHash]));
              const idx = items.findIndex((i) => i.entity_hash === exitHash);
              if (idx >= 0) lastClickedIndexRef.current = idx;
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
      <BatchRenamePanel />
    </div>
  );
}
