import { useEffect, useRef, useState, useCallback } from 'react';
import { useAtomValue, useSetAtom, getDefaultStore } from 'jotai';
import { IconPhoto, IconUpload, IconFolderPlus } from '@tabler/icons-react';
import * as entityMutations from '../../controllers/entityMutations';
import { getShortcut, matchesShortcutDef } from '../../shared/lib/shortcuts';
import { activeNodeIdAtom } from '../../state/navigation';
import {
  gridSessionAtom,
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
  loadedSelectedEntityHashesAtom,
  selectAllResultsAtom,
  selectionCountAtom,
  selectionTargetAtom,
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
import { SubfolderGrid, type SubfolderGridHandle } from './SubfolderGrid';
import { ContextMenu, useContextMenu } from '../../shared/ui/ContextMenu';
import { buildTileContextMenu, buildEmptyContextMenu } from './gridContextMenu';
import { pushHistory } from '../../state/navigationHistory';
import { viewerSessionAtom, quickLookSessionAtom, createViewerSession, navigateViewerSession, resolveViewerIndex, type ViewerSession } from '../../state/viewer';
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

const store = getDefaultStore();
const STATUS_ACTIVE = 1;
const STATUS_TRASH = 2;
const EMPTY_COPY: Record<string, [string, string]> = {
  inbox: ['Inbox is empty', 'Run subscriptions to add new images to your inbox'],
  uncategorized: ['No uncategorized images', 'All your images are already assigned to folders'],
  untagged: ['No untagged images', 'All your images have been tagged'],
  smart_folder: ['No matching images', 'Try adjusting the rules for this smart folder'],
  folder: ['This folder is empty', 'Drag and drop files here, or import them below'],
};

function useLatest<T>(value: T) {
  const ref = useRef(value);
  ref.current = value;
  return ref;
}

function supportsExplicitImageAutoTagging(
  querySelectionActive: boolean,
  hashes: Set<string>,
  items: Array<{ entity_hash: string; mime_type: string }>,
): boolean {
  if (querySelectionActive || hashes.size === 0) {
    return false;
  }
  const selectedItems = items.filter((item) => hashes.has(item.entity_hash));
  return (
    selectedItems.length === hashes.size &&
    selectedItems.every((item) => item.mime_type.startsWith('image/'))
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
  const session = useAtomValue(gridSessionAtom);
  const { items, cursor, totalCount, totalSizeBytes, searchText, scope: gridScope, error } = session;
  const { mode: viewMode, targetSize, showName, showExtension, showExtensionLabel,
    showResolution, fitThumbnails, showSubfolders } = session.view;
  const loading = session.status === 'loading';
  const sidebarNodes = useAtomValue(sidebarNodesAtom);
  const selection = useAtomValue(gridSelectionAtom);
  const selectedHashes = useAtomValue(loadedSelectedEntityHashesAtom);
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
  const itemsRef = useLatest(items);
  const selectionCountRef = useLatest(selectionCount);
  const selectedHashesRef = useLatest(selectedHashes);
  const querySelectionActiveRef = useLatest(querySelectionActive);
  const viewerSessionRef = useLatest(viewerSession);
  const quickLookSessionRef = useLatest(quickLookSession);
  const gridScopeRef = useLatest(gridScope);
  const setTagSelectModalRef = useLatest(setTagSelectModal);
  const setFolderPickerModalRef = useLatest(setFolderPickerModal);
  const setAiTaggerPortalRef = useLatest(setAiTaggerPortal);

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
    selectedHashes,
    selection,
    dispatchSelection,
    viewerOpen: !!(viewerSession || quickLookSession),
    containerWidth: gridContainerRef.current?.clientWidth ?? 0,
    targetSize,
  });

  const [fileDragOver, setFileDragOver] = useState(false);

  useEffect(() => {
    const webview = (window as any).picto?.webview;
    if (!webview?.onDragDropEvent) return;

    const promise = webview.onDragDropEvent((event: { payload: { type: string; paths?: string[] } }) => {
      const { type, paths } = event.payload;
      if (isNativeDragPending() || isDragActiveCheck() || isInternalDragOrigin()) return;
      if (type === 'enter') { setFileDragOver(true); return; }
      if (type === 'leave') { setFileDragOver(false); return; }
      if (type !== 'drop' || !paths?.length) return;
      setFileDragOver(false);

      const scope = gridScopeRef.current;
      const folderId = scope.kind === 'folder' ? scope.id : null;

      const mediaExt = /\.(jpe?g|png|gif|webp|bmp|tiff?|svg|mp4|mkv|webm|avi|mov|wmv|flv|m4v|avif|jxl|ico|pdf)$/i;
      if (paths.length === 1 && !mediaExt.test(paths[0])) {
        store.set(folderImportModalAtom, {
          open: true,
          path: paths[0],
          targetFolderId: folderId ?? null,
          initialStatus: manualImportParamsForScope(scope).initial_status,
        });
      } else {
        void filesController.addMedia(paths, manualImportParamsForScope(scope,
          folderId != null ? { parent_folder_id: folderId } : {}));
      }
    });

    return () => { promise.then((fn: () => void) => fn()); };
  }, []);

  const childFolders = useAtomValue(gridChildFoldersAtom);
  const contextMenu = useContextMenu();

  const setDisplayedGridSnapshot = useSetAtom(displayedGridSnapshotAtom);
  const setDisplayedInspectorTarget = useSetAtom(displayedInspectorTargetAtom);
  const setDisplayedEntityData = useSetAtom(displayedInspectorEntityDataAtom);
  const setInspectorLoading = useSetAtom(inspectorLoadingAtom);
  const setInspectorError = useSetAtom(inspectorErrorAtom);
  const liveTarget = useAtomValue(liveInspectorTargetAtom);

  const lastScrollTopRef = useRef(0);

  // Commit at transition midpoint; idle commits are same-scope reconciliation.
  const displayedNodeIdRef = useRef(displayedNodeId);

  useEffect(() => {
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
    if (!target || gridScope.kind !== 'folder' || gridScope.id == null) return;
    await entityMutations.updateTargetFolderMembership(target, gridScope.id, 'remove');
  }, [gridScope, selectionTarget]);

  const setSelectionStatus = useCallback(async (status: number, target = selectionTarget) => {
    if (!target) return;
    await entityMutations.setTargetStatus(target, status);
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

  const importMedia = useCallback(async (directory: boolean) => {
    try {
      const result = await (window as any).picto.dialog.open(directory ? {
        properties: ['openDirectory'], multiple: false, title: 'Import folder',
      } : {
        properties: ['openFile'], multiple: true, title: 'Import files',
        filters: [{ name: 'Media', extensions: ['png','jpg','jpeg','gif','webp','bmp','mp4','webm','mkv','mov','avi'] }],
      });
      if (!result) return;
      const paths = Array.isArray(result) ? result : [result];
      await filesController.addMedia(paths, manualImportParamsForScope(gridScope, directory ? {
        preserve_structure: true,
        parent_folder_id: gridScope.kind === 'folder' ? gridScope.id : null,
      } : gridScope.kind === 'folder' ? { parent_folder_id: gridScope.id } : {}));
    } catch (error) {
      console.error(`[grid] import ${directory ? 'folder' : 'files'} failed:`, error);
    }
  }, [gridScope]);

  const detailWindowSelectionRef = useRef(new Map<string, string[]>());
  const openDetailWindow = useCallback((hash: string, hashes = [...selectedHashesRef.current]) => {
    const item = itemsRef.current.find((entry) => entry.entity_hash === hash);
    const label = `detail-${hash.slice(0, 12)}`;
    detailWindowSelectionRef.current.set(label, hashes);
    void windowController.openDetailWindow({
      hash,
      width: item?.pixel_width ?? null,
      height: item?.pixel_height ?? null,
    });
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
      if (viewerSessionRef.current || quickLookSessionRef.current) {
        if (!matchesShortcutDef(e, defs.detailView) && !matchesShortcutDef(e, defs.quicklook)) return;
      }
      const count = selectionCountRef.current;
      const hashes = selectedHashesRef.current;
      const curItems = itemsRef.current;
      const scope = gridScopeRef.current;
      const isTrash = scope.kind === 'system' && scope.key === 'trash';
      const singleHash = count === 1 ? [...hashes][0] : null;
      const canAutoTag = supportsExplicitImageAutoTagging(
        querySelectionActiveRef.current,
        hashes,
        curItems,
      );

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
        const selectedArr = [...hashes];
        openDetailWindow(singleHash ?? selectedArr[0], selectedArr);
        return;
      }

      if (matchesShortcutDef(e, defs.delete_) && count > 0) {
        e.preventDefault();
        if (isTrash) void permanentlyDeleteSelection();
        else void setSelectionStatus(STATUS_TRASH);
        return;
      }
      if (matchesShortcutDef(e, defs.restore) && count > 0) {
        e.preventDefault();
        if (isTrash) void setSelectionStatus(STATUS_ACTIVE);
        else if (scope.kind === 'folder') void removeSelectionFromCurrentFolder();
        return;
      }

      if (matchesShortcutDef(e, defs.addToFolder) && count > 0) {
        e.preventDefault(); void addSelectionToFolder(); return;
      }

      if (matchesShortcutDef(e, defs.addTag) && count > 0) {
        e.preventDefault(); setTagSelectModalRef.current({ open: true }); return;
      }
      if (matchesShortcutDef(e, defs.addToFolders) && count > 0) {
        e.preventDefault(); setFolderPickerModalRef.current({ open: true }); return;
      }
      if (matchesShortcutDef(e, defs.autoTag) && canAutoTag) {
        e.preventDefault(); setAiTaggerPortalRef.current({ open: true, anchor: inspectorAnchor() }); return;
      }

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
  }, [clearSelection, selectAllResults, setViewerSession, setQuickLookSession, setSelectionStatus, permanentlyDeleteSelection, addSelectionToFolder, removeSelectionFromCurrentFolder, openDetailWindow]);

  const isEmpty = items.length === 0 && !loading;

  const navigateOverlay = (
    session: ViewerSession,
    setSession: (next: ViewerSession | null) => void,
    delta: number,
    center: boolean,
  ) => {
    const next = navigateViewerSession(session, items, delta);
    if (!next) return;
    setSession(next);
    dispatchSelection({ type: 'replace_entities', hashes: new Set([next.currentHash]), anchor: next.currentHash });
    if (center) scrollToItem(items.findIndex((item) => item.entity_hash === next.currentHash), 'center');
  };

  const closeOverlay = (setSession: (next: ViewerSession | null) => void, exitHash: string | null) => {
    setSession(null);
    if (!exitHash) return;
    dispatchSelection({ type: 'replace_entities', hashes: new Set([exitHash]), anchor: exitHash });
    scrollToItem(items.findIndex((item) => item.entity_hash === exitHash));
  };

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
      const scopeKey = gridScope.kind === 'system' ? gridScope.key ?? 'all' : gridScope.kind;
      const hasSearch = searchText.trim().length > 0;
      const [emptyTitle, emptyDesc] = hasSearch
        ? ['No results found', 'Try different search terms or clear filters']
        : EMPTY_COPY[scopeKey] ?? ['No images', 'Drag and drop files here, or click the button below to import'];
      const showImport = !hasSearch && scopeKey !== 'inbox' && scopeKey !== 'untagged' && scopeKey !== 'smart_folder';

      return (
        <EmptyState
          icon={<IconPhoto size={28} stroke={1.2} style={{ color: 'var(--color-bg-app)', opacity: 1 }} />}
          title={emptyTitle}
          description={emptyDesc}
          actions={showImport ? (
            <>
              <EmptyStateAction onClick={() => void importMedia(false)}>
                <IconUpload size={14} stroke={1.5} />
                Import Files
              </EmptyStateAction>
              <EmptyStateAction onClick={() => void importMedia(true)}>
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
          if (!selectedSubfolderNodeIds.has(nodeId)) {
            dispatchSelection({ type: 'replace_folders', ids: new Set([nodeId]), anchor: nodeId });
          }
          const folderId = parseInt(nodeId.replace('folder:', ''), 10);
          if (isNaN(folderId)) return;
          const entries = buildTileContextMenu({
            selectionCount: 1,
            singleSelected: true,
            singleHash: nodeId,
            isMixed: false,
            isFoldersOnly: true,
            scopeKind: 'folder',
            statusFilter: null,
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
        totalCount={totalCount}
        interactive={!viewerSession && !quickLookSession}
        suppressTileReveal={transitionPhase === 'fading_out' || transitionPhase === 'waiting'}
        selectedEntityHashes={selectedHashes}
        selectedFolderNodeIds={selectedSubfolderNodeIds}
        initialScrollTop={initialScrollTop}
        onContainerRef={(el) => { gridContainerRef.current = el; }}
        onLayoutChange={(l) => { gridLayoutRef.current = l; }}
        renamingIndex={renamingIndex}
        onRenameCommit={(idx, name) => {
          setRenamingIndex(null);
          const item = items[idx];
          if (item && name) void entityMutations.setEntityName(item.entity_hash, name);
        }}
        onRenameCancel={() => setRenamingIndex(null)}
        onFirstPaint={onFirstPaint}
        onScrollTopChange={(scrollTop) => { lastScrollTopRef.current = scrollTop; onScrollTopChange?.(scrollTop); }}
        onTileClick={(index, item, event) => {
          const hash = item.entity_hash;
          if (event?.shiftKey && selection.anchor?.kind === 'entity') {
            const anchorIndex = items.findIndex((entry) => entry.entity_hash === selection.anchor!.id);
            const from = Math.min(anchorIndex >= 0 ? anchorIndex : index, index);
            const to = Math.max(anchorIndex >= 0 ? anchorIndex : index, index);
            const base = (event.metaKey || event.ctrlKey)
              ? new Set(selectedHashes)
              : new Set<string>();
            for (let i = from; i <= to; i++) {
              if (items[i]) base.add(items[i].entity_hash);
            }
            dispatchSelection({ type: 'range_entities', hashes: base });
          } else if (event?.metaKey || event?.ctrlKey) {
            dispatchSelection(selectionMode === 'query_results'
              ? { type: 'toggle_query_entity', hash, totalCount: totalCount ?? items.length }
              : { type: 'toggle_entity', hash });
          } else {
            dispatchSelection({ type: 'replace_entities', hashes: new Set([hash]), anchor: hash });
          }
        }}
        onTileDoubleClick={(_index, item) => {
          setViewerSession(createViewerSession(items, item.entity_hash));
        }}
        onEmptyClick={() => clearSelection()}
        onSelectionChange={(hashes) => dispatchSelection({ type: 'replace_entities', hashes })}
        onMarqueeSelectionChange={({ entityHashes, folderNodeIds }) => {
          dispatchSelection({ type: 'marquee', entityHashes, folderNodeIds, additive: false });
        }}
        collectHeaderMarqueeHits={(rect) => subfolderGridRef.current?.collectMarqueeHits(rect) ?? new Set()}
        onTileContextMenu={(_index, item, pos) => {
          let effectiveHashes = selectedHashes;
          let effectiveSelectionMode = selectionMode;
          let effectiveSelectionCount = selectionCount;
          let effectiveQuerySelectionActive = querySelectionActive;
          if (!selectedHashes.has(item.entity_hash)) {
            effectiveHashes = new Set([item.entity_hash]);
            dispatchSelection({ type: 'replace_entities', hashes: effectiveHashes, anchor: item.entity_hash });
            effectiveSelectionMode = 'explicit';
            effectiveSelectionCount = 1;
            effectiveQuerySelectionActive = false;
          }

          const selCount = effectiveSelectionCount;
          const selectedItems = items.filter((it) => effectiveHashes.has(it.entity_hash));
          const singleItem = effectiveSelectionMode === 'explicit' && selCount === 1 ? selectedItems[0] : null;
          const canAutoTag = effectiveSelectionMode === 'explicit'
            && selectedItems.length === effectiveHashes.size
            && selectedItems.every((selected) => selected.mime_type.startsWith('image/'));
          const effectiveTarget = resolveContextMenuTarget(
            effectiveQuerySelectionActive,
            selectionTarget,
            effectiveHashes,
          );
          const scopeKind = gridScope.kind === 'system' ? 'system'
            : gridScope.kind === 'folder' ? 'folder'
            : gridScope.kind === 'smart_folder' ? 'smart_folder'
            : null;
          const statusFilter = gridScope.kind === 'system'
            ? (gridScope.key === 'inbox' ? 'inbox' : gridScope.key === 'trash' ? 'trash' : gridScope.key === 'all' ? 'active' : null)
            : null;

          const entries = buildTileContextMenu({
            selectionCount: selCount,
            aiTagEnabled: canAutoTag,
            singleSelected: effectiveSelectionMode === 'explicit' && selCount === 1,
            singleHash: singleItem?.entity_hash ?? null,
            scopeKind,
            statusFilter,
            onSelectAll: () => selectAllResults(),
            onDeselectAll: () => clearSelection(),
            onOpen: singleItem ? () => setViewerSession(createViewerSession(items, singleItem.entity_hash)) : undefined,
            onOpenNewWindow: (hash) => openDetailWindow(hash, [...effectiveHashes]),
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
            onMoveToTrash: () => { void setSelectionStatus(STATUS_TRASH, effectiveTarget); },
            onRestore: () => { void setSelectionStatus(STATUS_ACTIVE, effectiveTarget); },
            onPermanentDelete: () => { permanentlyDeleteSelection(effectiveTarget, selCount); },
            onAccept: () => { void setSelectionStatus(STATUS_ACTIVE, effectiveTarget); },
            onReject: () => { void setSelectionStatus(STATUS_TRASH, effectiveTarget); },
          });
          contextMenu.openAt(pos, entries);
        }}
        onEmptyContextMenu={(pos) => {
          const entries = buildEmptyContextMenu({
            selectionCount,
            singleSelected: selectionCount === 1,
            singleHash: selectionCount === 1 ? [...selectedHashes][0] ?? null : null,
            scopeKind: null,
            statusFilter: null,
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
          onNavigate={(delta) => navigateOverlay(viewerSession, setViewerSession, delta, false)}
          onClose={(exitHash) => closeOverlay(setViewerSession, exitHash)}
          onLoadMore={cursor ? () => gridController.loadNextPage() : undefined}
        />
      )}

      {quickLookSession && (
        <QuickLook
          items={items}
          currentIndex={resolveViewerIndex(quickLookSession, items)}
          totalCount={totalCount}
          onNavigate={(delta) => navigateOverlay(quickLookSession, setQuickLookSession, delta, true)}
          onClose={(exitHash) => closeOverlay(setQuickLookSession, exitHash)}
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
