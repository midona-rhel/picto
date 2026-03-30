/**
 * Grid screen — feature root. Reads state, delegates to CanvasGrid.
 */

import { useEffect, useRef, useState, useCallback } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import { IconPhoto, IconUpload, IconFolderPlus } from '@tabler/icons-react';
import * as entityMutations from '../../controllers/entityMutations';
import { activeNodeIdAtom, subscriptionsWorkspaceTabAtom } from '../../state/navigation';
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
import { saveScrollPosition, getScrollPosition } from '../../state/navigationHistory';
import { promptForFolderId } from '../../shared/lib/selectFolderPrompt';
import { resolveFilePath, shellOpenPath, shellShowInFolder, clipboardWriteText, clipboardCopyFile } from '../../platform/api';
import { viewerSessionAtom, quickLookSessionAtom, createViewerSession, navigateViewerSession } from '../../state/viewer';
import { MediaView } from '../viewer/MediaView';
import { SubscriptionsScreen } from '../subscriptions/SubscriptionsScreen';
import { AuthWorkspace } from '../auth/AuthWorkspace';
import { QuickLook } from '../viewer/QuickLook';
import styles from './GridScreen.module.css';

const GRID_SYSTEM_SCOPES: Record<string, string> = {
  'system:active': 'all',
  'system:inbox': 'inbox',
  'system:trash': 'trash',
  'system:uncategorized': 'uncategorized',
  'system:untagged': 'untagged',
};

const NON_GRID_NODES = new Set(['system:duplicates', 'system:recent_viewed', 'system:subscriptions']);
const SCOPE_TRANSITION_MS = 250;
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
  if (NON_GRID_NODES.has(nodeId)) return null;
  const scopeKey = GRID_SYSTEM_SCOPES[nodeId];
  if (scopeKey) return { kind: 'system', key: scopeKey };
  return null;
}

export function GridScreen() {
  const activeNodeId = useAtomValue(activeNodeIdAtom);
  const subscriptionsWorkspaceTab = useAtomValue(subscriptionsWorkspaceTabAtom);
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
  const contextMenu = useContextMenu();

  // Reset range anchor when items change (scope nav, sort, search, reload)
  useEffect(() => { lastClickedIndexRef.current = null; }, [items]);
  const setDisplayedGridSnapshot = useSetAtom(displayedGridSnapshotAtom);
  const setDisplayedInspectorTarget = useSetAtom(displayedInspectorTargetAtom);
  const setDisplayedEntityData = useSetAtom(displayedInspectorEntityDataAtom);
  const setInspectorLoading = useSetAtom(inspectorLoadingAtom);
  const setInspectorError = useSetAtom(inspectorErrorAtom);
  const liveTarget = useAtomValue(liveInspectorTargetAtom);

  const [transitionPhase, setTransitionPhaseRaw] = useState<'idle' | 'fading_out' | 'waiting' | 'fading_in'>('idle');
  const setGridTransitionPhase = useSetAtom(gridTransitionPhaseAtom);
  const setTransitionPhase = useCallback((phase: 'idle' | 'fading_out' | 'waiting' | 'fading_in' | ((prev: 'idle' | 'fading_out' | 'waiting' | 'fading_in') => 'idle' | 'fading_out' | 'waiting' | 'fading_in')) => {
    setTransitionPhaseRaw((prev) => {
      const next = typeof phase === 'function' ? phase(prev) : phase;
      if (next !== prev) setGridTransitionPhase(next);
      return next;
    });
  }, [setGridTransitionPhase]);
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
      // Grid-to-grid: fade out old → wait → load new → fade in
      saveScrollPosition(previousNodeIdRef.current, lastScrollTopRef.current);
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

  // ── Grid keyboard shortcuts ──
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;

      if ((e.metaKey || e.ctrlKey) && e.key === 'a') {
        e.preventDefault();
        selectAllResults();
      }

      // Escape: clear selection
      if (e.key === 'Escape' && selectionCount > 0) {
        clearSelection();
      }

      // Enter: open viewer for single selected entity
      if (e.key === 'Enter' && selectionCount === 1 && !viewerSession && !quickLookSession) {
        const hash = [...selectedHashes][0];
        if (hash) {
          e.preventDefault();
          setViewerSession(createViewerSession(items, hash));
        }
      }

      // Space: toggle QuickLook for single selected entity
      if (e.key === ' ' && !viewerSession) {
        e.preventDefault();
        if (quickLookSession) {
          setQuickLookSession(null);
        } else if (selectionCount === 1) {
          const hash = [...selectedHashes][0];
          if (hash) setQuickLookSession(createViewerSession(items, hash));
        }
      }
    }
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [clearSelection, selectAllResults, selectionCount, selectedHashes, items, viewerSession, setViewerSession, quickLookSession, setQuickLookSession]);

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

  const incomingHidden = transitionPhase === 'waiting';
  const incomingFadingOut = transitionPhase === 'fading_out';
  const incomingFadingIn = transitionPhase === 'fading_in';
  const isEmpty = items.length === 0 && !loading;

  const renderIncomingSurface = () => {
    if (!isGridScope) {
      if (activeNodeId === 'system:subscriptions') {
        return subscriptionsWorkspaceTab === 'auth' ? <AuthWorkspace /> : <SubscriptionsScreen />;
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
            statusFilter,
            loadedCount: items.length,
            onSelectAll: () => selectAllResults(),
            onDeselectAll: () => clearSelection(),
            onOpen: singleItem ? () => setViewerSession(createViewerSession(items, singleItem.entity_hash)) : undefined,
            onOpenDefault: (hash) => { void resolveFilePath(hash).then((p) => { if (p) shellOpenPath(p); }); },
            onRevealInFolder: (hash) => { void resolveFilePath(hash).then((p) => { if (p) shellShowInFolder(p); }); },
            onCopyFilePath: (hash) => { void resolveFilePath(hash).then((p) => { if (p) clipboardWriteText(p); }); },
            onCopyFile: (hash) => { void resolveFilePath(hash).then((p) => { if (p) clipboardCopyFile(p); }); },
            onAddToFolder: () => { void addSelectionToFolder(); },
            onRemoveFromFolder: () => { void removeSelectionFromCurrentFolder(); },
            onMoveToTrash: () => { void setSelectionStatus(STATUS_TRASH); },
            onRestore: () => { void setSelectionStatus(STATUS_ACTIVE); },
            onPermanentDelete: () => { void permanentlyDeleteSelection(); },
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
    </div>
  );
}
