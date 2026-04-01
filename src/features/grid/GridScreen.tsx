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
} from '../../state/grid';
import { gridController } from '../../controllers/gridController';
import {
  clearSelectionAtom,
  querySelectionActiveAtom,
  selectAllResultsAtom,
  selectedEntityHashesAtom,
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
} from '../../state/inspector';
import { sidebarNodesAtom } from '../../state/sidebar';
import { CanvasGrid } from './canvas/CanvasGrid';
import { ContextMenu, useContextMenu } from '../../shared/ui/ContextMenu';
import { buildTileContextMenu, buildEmptyContextMenu } from './gridContextMenu';
import type { BaseScope } from '../../shared/types/canonical';
import { saveScrollPosition, getScrollPosition, pushHistory } from '../../state/navigationHistory';
import { promptForFolderId } from '../../shared/lib/selectFolderPrompt';
import { resolveFilePath, shellOpenPath, shellShowInFolder, clipboardWriteText, clipboardCopyFile, createCollection, addCollectionMembers, removeCollectionMembers, deleteCollection, listCollectionMemberHashes } from '../../platform/api';
import { viewerSessionAtom, quickLookSessionAtom, createViewerSession, navigateViewerSession } from '../../state/viewer';
import { tagSelectOpenAtom, folderPickerOpenAtom, aiTaggerOpenAtom, batchRenameOpenAtom } from '../../state/portals';
import { MediaView } from '../viewer/MediaView';
import { SubscriptionsScreen } from '../subscriptions/SubscriptionsScreen';
import { QuickLook } from '../viewer/QuickLook';
import { TagSelectPanel } from '../tags/TagSelectPanel';
import { FolderPickerPanel } from '../folders/FolderPickerPanel';
import { AiTaggerPanel } from '../ai-tagger/AiTaggerPanel';
import { BatchRenamePanel } from '../batch-rename/BatchRenamePanel';
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

const GRID_SYSTEM_SCOPES: Record<string, string> = {
  'system:active': 'all',
  'system:inbox': 'inbox',
  'system:trash': 'trash',
  'system:uncategorized': 'uncategorized',
  'system:untagged': 'untagged',
};

const NON_GRID_NODES = new Set(['system:duplicates', 'system:recent_viewed', 'system:subscriptions']);
const store = getDefaultStore();
const SCOPE_TRANSITION_MS = 170;
const STATUS_ACTIVE = 1;
const STATUS_TRASH = 2;

function nodeIdToScope(nodeId: string): BaseScope | null {
  if (nodeId.startsWith('folder:')) {
    const id = parseInt(nodeId.slice(7), 10);
    return { kind: 'folder', id: isNaN(id) ? 0 : id };
  }
  if (nodeId.startsWith('smart:')) {
    const id = parseInt(nodeId.slice(6), 10);
    return { kind: 'smart_folder', id: isNaN(id) ? 0 : id };
  }
  if (nodeId.startsWith('collection:')) {
    const id = parseInt(nodeId.slice(11), 10);
    return { kind: 'collection', id: isNaN(id) ? 0 : id };
  }
  if (NON_GRID_NODES.has(nodeId)) return null;
  const scopeKey = GRID_SYSTEM_SCOPES[nodeId];
  if (scopeKey) return { kind: 'system', key: scopeKey };
  return null;
}

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
  const sidebarNodes = useAtomValue(sidebarNodesAtom);
  const selectedHashes = useAtomValue(selectedEntityHashesAtom);
  const selectionMode = useAtomValue(selectionModeAtom);
  const querySelectionActive = useAtomValue(querySelectionActiveAtom);
  const selectionCount = useAtomValue(selectionCountAtom);
  const selectionTarget = useAtomValue(selectionTargetAtom);
  const setSelectedHashes = useSetAtom(selectedEntityHashesAtom);
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
  const contextMenu = useContextMenu();

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

  const scope = nodeIdToScope(activeNodeId);
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
    const previousScope = nodeIdToScope(previousNodeIdRef.current);
    const nextScope = nodeIdToScope(activeNodeId);
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
        void gridController.navigateTo(nodeIdToScope(activeNodeId)!);
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
        void gridController.navigateTo(nodeIdToScope(committedNodeId)!);
        previousNodeIdRef.current = committedNodeId;
      }, SCOPE_TRANSITION_MS);
      return;
    }

    // Non-grid to grid: no fade-out, start in waiting for clean fade-in
    setTransitionPhase('waiting');
    restoredScrollTopRef.current = getScrollPosition(activeNodeId);
    void gridController.navigateTo(nextScope);
    previousNodeIdRef.current = activeNodeId;
  }, [activeNodeId, clearTransition]);

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

    // During fading_in: always commit (new scope arriving after transition)
    // During idle: only commit if we're on the SAME scope (data update, not scope change)
    const isSameScope = activeNodeId === displayedNodeIdRef.current;
    const shouldCommit = transitionPhase === 'fading_in' || (transitionPhase === 'idle' && isSameScope);

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
      if (transitionPhase === 'fading_in' || liveTarget.kind === 'scope' || liveTarget.kind === 'none') {
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



  const addSelectionToFolder = useCallback(async () => {
    if (!selectionTarget) return;
    const folderId = promptForFolderId(sidebarNodes);
    if (folderId != null) {
      await entityMutations.updateTargetFolderMembership(selectionTarget, folderId, 'add');
    }
  }, [selectionTarget, sidebarNodes]);

  const removeSelectionFromCurrentFolder = useCallback(async () => {
    if (!selectionTarget || gridScope.kind !== 'folder' || gridScope.id == null) return;
    await entityMutations.updateTargetFolderMembership(selectionTarget, gridScope.id, 'remove');
  }, [gridScope, selectionTarget]);

  const setSelectionStatus = useCallback(async (status: number) => {
    if (!selectionTarget) return;
    await entityMutations.setTargetStatus(selectionTarget, status);
  }, [selectionTarget]);

  const permanentlyDeleteSelection = useCallback(async () => {
    if (!selectionTarget) return;
    await entityMutations.permanentlyDeleteTarget(selectionTarget);
    clearSelection();
  }, [selectionTarget, clearSelection]);

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
    };

    function handleKeyDown(e: KeyboardEvent) {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
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
        e.preventDefault(); void resolveFilePath(singleHash).then((p) => { if (p) shellOpenPath(p); }); return;
      }
      if (matchesShortcutDef(e, defs.revealInFolder) && singleHash) {
        e.preventDefault(); void resolveFilePath(singleHash).then((p) => { if (p) shellShowInFolder(p); }); return;
      }
      if (matchesShortcutDef(e, defs.openNewWindow) && count > 0) {
        e.preventDefault();
        // Use first selected hash as the window identity
        const selectedArr = [...hashes];
        const primaryHash = singleHash ?? selectedArr[0];
        const item = curItems.find((i) => i.entity_hash === primaryHash);
        const label = `detail-${primaryHash.slice(0, 12)}`;
        detailWindowSelectionRef.current.set(label, selectedArr);
        void (window as any).picto.api.invoke('open_in_new_window', {
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
              <button className={styles.emptyBtn} type="button" onClick={() => { /* TODO: import files */ }}>
                <IconUpload size={14} stroke={1.5} />
                Import Files
              </button>
              <button className={styles.emptyBtn} type="button" onClick={() => { /* TODO: import folder */ }}>
                <IconFolderPlus size={14} stroke={1.5} />
                Import Folder
              </button>
            </div>
          )}
        </div>
      );
    }

    return (
      <CanvasGrid
        items={items}
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
        onFirstPaint={() => { restoredScrollTopRef.current = null; beginFadeIn(); }}
        onScrollTopChange={(scrollTop) => { lastScrollTopRef.current = scrollTop; }}
        onTileClick={(index, item, event) => {
          const hash = item.entity_hash;
          if (event?.shiftKey && lastClickedIndexRef.current != null) {
            // Shift+click: range select. Intentionally exits query_results mode
            // because range selection defines an explicit set of items.
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
            // Plain click: single select (intentionally exits query_results mode)
            setSelectedHashes(new Set([hash]));
            lastClickedIndexRef.current = index;
          }
        }}
        onTileDoubleClick={(_index, item) => {
          if (item.entity_kind === 'collection') {
            const collNodeId = `collection:${item.entity_id}`;
            setParentNodeId(activeNodeId);
            setCollectionName(item.name);
            pushHistory(collNodeId);
            setActiveNodeId(collNodeId);
          } else {
            setViewerSession(createViewerSession(items, item.entity_hash));
          }
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
              void (window as any).picto.api.invoke('open_in_new_window', {
                hash, width: it?.pixel_width ?? null, height: it?.pixel_height ?? null,
              });
            },
            onOpenDefault: (hash) => { void resolveFilePath(hash).then((p) => { if (p) shellOpenPath(p); }); },
            onRevealInFolder: (hash) => { void resolveFilePath(hash).then((p) => { if (p) shellShowInFolder(p); }); },
            onCopyFilePath: (hash) => { void resolveFilePath(hash).then((p) => { if (p) clipboardWriteText(p); }); },
            onCopyFile: (hash) => { void resolveFilePath(hash).then((p) => { if (p) clipboardCopyFile(p); }); },
            onAddToFolder: () => { setFolderPickerOpen(true); },
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
                const id = await createCollection(name);
                await addCollectionMembers(id, hashes);
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
              ? () => { void removeCollectionMembers(gridScope.id!, [...effectiveHashes]); }
              : undefined,
            onSplitCollection: (() => {
              // Inside collection view — split and navigate back instantly
              if (gridScope.kind === 'collection' && gridScope.id != null) {
                return () => {
                  void (async () => {
                    const memberHashes = await listCollectionMemberHashes(gridScope.id!);
                    await deleteCollection(gridScope.id!);
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
                    const memberHashes = await listCollectionMemberHashes(singleItem.entity_id);
                    await deleteCollection(singleItem.entity_id);
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

      {viewerSession && (
        <MediaView
          items={items}
          currentIndex={viewerSession.currentIndex}
          totalCount={totalCount}
          onNavigate={(delta) => {
            const next = navigateViewerSession(viewerSession, items, delta);
            if (next) setViewerSession(next);
          }}
          onClose={() => setViewerSession(null)}
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
            if (next) setQuickLookSession(next);
          }}
          onClose={() => setQuickLookSession(null)}
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
