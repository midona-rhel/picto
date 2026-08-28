/**
 * Sidebar feature root — assembles the sidebar from state atoms.
 *
 * All nodes are driven by the backend sidebar tree. No frontend-invented nodes.
 * Manager surfaces (Tags, Random) are out of scope — see PBI-595, PBI-596.
 */

import { useEffect, useMemo, useCallback, useRef, useState, type ReactNode } from 'react';
import { useAtomValue, useSetAtom, getDefaultStore } from 'jotai';
import { folderWatchModalAtom, confirmModalAtom, exportModalAtom, smartFolderModalAtom } from '../../state/modals';
import {
  IconFolder, IconFolderOpen, IconFolderPlus,
  IconCopy, IconDownload,
  IconPhoto, IconInbox, IconTrash,
  IconClock, IconBookmark,
  IconArrowsShuffle,
  IconLayoutGrid, IconRefresh, IconX, IconFilterPlus,
  IconAdjustments,
  IconStar, IconStarOff,
} from '@tabler/icons-react';
import type { Icon as TablerIcon } from '@tabler/icons-react';
import {
  IconNewSubfolder, IconRename, IconSort, IconExpand, IconCollapse,
  IconExpandAll, IconCollapseAll, IconChangeIcon, IconWatchFolder,
  IconFolderQuestionCustom, IconBookmarkQuestionCustom, IconAutoTags,
} from '../../shared/ui/icons/sidebar-menu-icons';
import {
  sidebarNodesAtom, systemNodesAtom, folderNodesAtom,
  smartFolderNodesAtom, sidebarLoadingAtom,
  pendingSidebarRenameNodeIdAtom,
} from '../../state/sidebar';
import { displayedSurfaceNodeIdAtom, sidebarPreferencesAtom } from '../../state/navigation';
import { navigateToNode } from '../../state/navigationHistory';
import { sidebarController } from '../../controllers/sidebarController';
import { settingsController, type AppSettings } from '../../controllers/settingsController';
import {
  bulkFolderDeletionMessage,
  foldersController,
  singleFolderDeletionMessage,
} from '../../controllers/foldersController';
import { smartFoldersController } from '../../controllers/smartFoldersController';
import { SidebarRow } from '../../shared/ui/SidebarRow';
import { LibrarySwitcherButton } from '../library/LibrarySwitcherButton';
import { ContextMenu, useContextMenu, type MenuEntry } from '../../shared/ui/ContextMenu';
import { buildExportContextEntry } from '../grid/gridContextMenu';
import { ColorPicker } from '../../shared/ui/ColorPicker';
import { IconPicker } from '../../shared/ui/IconPicker';
import { DynamicIcon } from '../../shared/ui/DynamicIcon';
import { ToolbarFilterIcon } from '../../shared/ui/icons/toolbar-icons';
import { useInlineRename } from '../../shared/hooks/useInlineRename';
import { usePersistedSet } from '../../shared/hooks/usePersistedSet';
import type { BaseScope, SidebarNodeDto, SmartFolderCommandPayload, SmartFolderPredicate } from '../../shared/types/canonical';
import type { EntityTarget } from '../../shared/types/canonical';
import { filterSidebarTree } from './treeFilter';
import * as entityMutations from '../../controllers/entityMutations';
import { clearRecentViews } from '../../platform/entityApi';
import { compileGridQuery, createEmptyItemFilters } from '../../shared/lib/itemFilters';
import { folderPickerPortalAtom } from '../../state/portals';
import { announceUndoableMutation } from '../../runtime/historyRuntime';
import { formatKeysDisplay, getShortcut, matchesShortcutDef } from '../../shared/lib/shortcuts';
import { useShortcutScope } from '../../shared/hooks/useShortcutScope';
import {
  addQuickAccess,
  removeQuickAccess,
  reorderQuickAccess,
  useQuickAccess,
} from './quickAccessPreferences';
import styles from './Sidebar.module.css';
import { chooseAndImportFolder, filesController } from '../../controllers/filesController';
import { showErrorNotification } from '../../shared/lib/notifications';
import { subscriptionsWorkspaceSnapshotAtom } from '../../state/subscriptionsWorkspace';
import { openFolderAutoTagsEditor } from '../folders/folderAutoTagsWorkflow';

const IC = 19;
const FILL = { stroke: 1.2, fill: 'currentColor', fillOpacity: 0.15 } as const;

function kbd(id: string): string | undefined {
  const shortcut = getShortcut(id);
  return shortcut ? formatKeysDisplay(shortcut.keys) : undefined;
}

const SYSTEM_ICONS: Record<string, TablerIcon> = {
  'system:active':        IconPhoto,
  'system:inbox':         IconInbox,
  'system:uncategorized': IconFolderQuestionCustom as unknown as TablerIcon,
  'system:untagged':      IconBookmarkQuestionCustom as unknown as TablerIcon,
  'system:tag_manager':   IconBookmark,
  'system:random':        IconArrowsShuffle,
  'system:recent_viewed': IconClock,
  'system:subscriptions': IconDownload,
  'system:duplicates':    IconCopy,
  'system:trash':         IconTrash,
};

const store = getDefaultStore();

function scopeExportEntry(target: EntityTarget, fileCount: number): MenuEntry {
  return buildExportContextEntry({
    onExportOriginals: () => {
      void filesController.chooseAndExportOriginals(target).catch((reason) => showErrorNotification({
        title: 'Could not export originals',
        message: reason instanceof Error ? reason.message : String(reason),
      }));
    },
    onExportAs: () => store.set(exportModalAtom, { open: true, fileCount, target }),
  });
}

export function resolveSidebarTreeDrop(
  element: HTMLElement | null,
  pointerY: number,
  draggedNodeId: string,
  nodes: SidebarNodeDto[],
): { targetId: string; position: 'before' | 'inside' | 'after' } | null {
  if (!element) return null;
  const smart = draggedNodeId.startsWith('smart:');
  const attr = smart ? 'data-smart-drop-id' : 'data-folder-drop-id';
  const row = element.closest<HTMLElement>(`[${attr}]`);
  if (!row) return null;
  const rawId = smart ? row.dataset.smartDropId : row.dataset.folderDropId;
  const targetId = rawId ? `${smart ? 'smart:' : 'folder:'}${rawId}` : null;
  if (!targetId || targetId === draggedNodeId || isDescendantOf(targetId, draggedNodeId, nodes)) return null;
  const rect = row.getBoundingClientRect();
  const ratio = (pointerY - rect.top) / rect.height;
  return {
    targetId,
    position: ratio < 0.3 ? 'before' : ratio > 0.7 ? 'after' : 'inside',
  };
}

export function availableFolderMoveTargets(nodes: SidebarNodeDto[], movingFolderId: number): number[] {
  const parentById = new Map<number, number | null>();
  for (const node of nodes) {
    const id = parseFolderId(node.id);
    if (id == null) continue;
    parentById.set(id, parseFolderId(node.parent_id ?? ''));
  }
  return [...parentById.keys()].filter((candidate) => {
    if (candidate === movingFolderId) return false;
    let ancestor: number | null | undefined = candidate;
    while (ancestor != null) {
      if (ancestor === movingFolderId) return false;
      ancestor = parentById.get(ancestor);
    }
    return true;
  });
}

export function availableTreeMoveTargetIds(
  nodes: SidebarNodeDto[],
  movingNodeIds: readonly string[],
): string[] {
  return nodes
    .map((node) => node.id)
    .filter((candidate) => movingNodeIds.every((movingId) => (
      candidate !== movingId && !isDescendantOf(candidate, movingId, nodes)
    )));
}

/** Fixed display order for all system scopes. */
const SYSTEM_SCOPE_ORDER = [
  'system:active',
  'system:inbox',
  'system:recent_viewed',
  'system:uncategorized',
  'system:untagged',
  'system:tag_manager',
  'system:random',
  'system:subscriptions',
  'system:duplicates',
  'system:trash',
];

const LABEL_OVERRIDES: Record<string, string> = {
  'system:active': 'All',
};

const SYSTEM_HELP_IDS: Record<string, string> = {
  'system:active': 'sidebar-all',
  'system:inbox': 'sidebar-inbox',
  'system:recent_viewed': 'sidebar-recently-viewed',
  'system:uncategorized': 'sidebar-uncategorized',
  'system:untagged': 'sidebar-untagged',
  'system:tag_manager': 'sidebar-tags',
  'system:random': 'sidebar-random',
  'system:subscriptions': 'sidebar-subscriptions',
  'system:duplicates': 'sidebar-duplicates',
  'system:trash': 'sidebar-trash',
};

const EXPAND_FILTERED_TREE = new Set<string>();

export function nextSidebarSelection(
  current: ReadonlySet<string>,
  clickedId: string,
  mode: 'replace' | 'toggle' | 'range',
  rangeIds: readonly string[] = [],
): Set<string> {
  if (mode === 'replace') return new Set([clickedId]);
  const next = new Set(current);
  if (mode === 'toggle') {
    if (next.has(clickedId)) next.delete(clickedId);
    else next.add(clickedId);
    return next;
  }
  for (const id of rangeIds) next.add(id);
  return next;
}

function queryTarget(scope: BaseScope): EntityTarget {
  return {
    kind: 'query',
    query: compileGridQuery(
      scope,
      createEmptyItemFilters(),
      { field: 'imported_at', direction: 'descending', random_seed: null },
    ),
    excluded_root_ids: [],
  };
}

export function Sidebar() {
  const nodes = useAtomValue(sidebarNodesAtom);
  const systemNodes = useAtomValue(systemNodesAtom);
  const folderNodes = useAtomValue(folderNodesAtom);
  const smartFolderNodes = useAtomValue(smartFolderNodesAtom);
  const loading = useAtomValue(sidebarLoadingAtom);
  const pendingRenameNodeId = useAtomValue(pendingSidebarRenameNodeIdAtom);
  const setPendingRenameNodeId = useSetAtom(pendingSidebarRenameNodeIdAtom);
  const activeNodeId = useAtomValue(displayedSurfaceNodeIdAtom);
  const sidebarPreferences = useAtomValue(sidebarPreferencesAtom);
  const setSidebarPreferences = useSetAtom(sidebarPreferencesAtom);
  const quickAccessIds = useQuickAccess();
  const setSmartFolderModal = useSetAtom(smartFolderModalAtom);
  const setFolderPortal = useSetAtom(folderPickerPortalAtom);
  const subscriptionsSnapshot = useAtomValue(subscriptionsWorkspaceSnapshotAtom);
  const subscriptionsRunning = (subscriptionsSnapshot?.runningSubscriptionIds.length ?? 0) > 0;

  const [collapsed, toggleCollapse] = usePersistedSet('picto-sidebar-collapsed');
  const [treeFilter, setTreeFilter] = useState('');
  const contextMenu = useContextMenu();
  const [sidebarVisibilityMenuOpen, setSidebarVisibilityMenuOpen] = useState(false);

  // ── Multi-select state ──
  const [sidebarSelection, setSidebarSelection] = useState<Set<string>>(new Set());
  const lastClickedRef = useRef<string | null>(null);

  // ── Context menu highlight ──
  const [contextMenuNodeId, setContextMenuNodeId] = useState<string | null>(null);
  // Clear highlight when menu closes
  useEffect(() => {
    if (!contextMenu.state && contextMenuNodeId) setContextMenuNodeId(null);
  }, [contextMenu.state, contextMenuNodeId]);

  // ── Folder drag reorder ──
  const folderDragRef = useRef<{ nodeId: string; startY: number } | null>(null);
  const [folderDragState, setFolderDragState] = useState<{
    active: boolean;
    draggedNodeId: string;
    dropTargetId: string | null;
    dropPosition: 'before' | 'inside' | 'after' | null;
    ghostX: number;
    ghostY: number;
    ghostLabel: string;
    ghostIcon: string | null;
    ghostColor: string | null;
  } | null>(null);

  // Global pointer listeners for folder/smart folder drag
  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      const drag = folderDragRef.current;
      if (!drag) return;
      const dy = e.clientY - drag.startY;
      if (!folderDragState?.active && Math.abs(dy) < 5) return;

      const isSmartDrag = drag.nodeId.startsWith('smart:');
      const nodePool = isSmartDrag ? smartFolderNodes : folderNodes;

      // Activate drag
      const node = [...folderNodes, ...smartFolderNodes].find((n) => n.id === drag.nodeId);
      if (!folderDragState?.active) {
        document.documentElement.setAttribute('data-sidebar-drag-active', 'true');
        setFolderDragState({
          active: true,
          draggedNodeId: drag.nodeId,
          dropTargetId: null,
          dropPosition: null,
          ghostX: e.clientX,
          ghostY: e.clientY,
          ghostLabel: sidebarSelection.has(drag.nodeId) && sidebarSelection.size > 1
            ? `${sidebarSelection.size} items`
            : (node?.name ?? (isSmartDrag ? 'Smart Folder' : 'Folder')),
          ghostIcon: node?.icon ?? null,
          ghostColor: node?.color ?? null,
        });
        document.body.style.cursor = 'grabbing';
      } else {
        // Find drop target via elementFromPoint
        const resolved = resolveSidebarTreeDrop(
          document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null,
          e.clientY,
          drag.nodeId,
          nodePool,
        );

        setFolderDragState((prev) => prev ? {
          ...prev,
          ghostX: e.clientX,
          ghostY: e.clientY,
          dropTargetId: resolved?.targetId ?? null,
          dropPosition: resolved?.position ?? null,
        } : null);
      }
    };

    const onUp = () => {
      const drag = folderDragRef.current;
      folderDragRef.current = null;
      document.body.style.cursor = '';
      document.documentElement.removeAttribute('data-sidebar-drag-active');

      if (folderDragState?.active && drag) {
        const { dropTargetId, dropPosition } = folderDragState;
        const isSmartDrag = drag.nodeId.startsWith('smart:');

        if (dropTargetId && dropPosition) {
          if (isSmartDrag) {
            // Smart folder drag-drop — multi-select aware
            const draggedId = parseSmartFolderIdNum(drag.nodeId);
            const targetId = parseSmartFolderIdNum(dropTargetId);
            if (draggedId != null && targetId != null) {
              const targetNode = smartFolderNodes.find((n) => n.id === dropTargetId);
              const rawIds = sidebarSelection.has(drag.nodeId) && sidebarSelection.size > 1
                ? [...sidebarSelection].filter((id) => id.startsWith('smart:'))
                : [drag.nodeId];
              const movingIds = deduplicateParentChild(rawIds, smartFolderNodes);
              const movingSet = new Set(movingIds);

              if (dropPosition === 'inside') {
                for (const id of movingIds) {
                  const sfId = parseSmartFolderIdNum(id);
                  if (sfId != null) void smartFoldersController.move(sfId, targetId, []);
                }
              } else {
                const targetParentId = targetNode?.parent_id ?? null;
                const parentSmartId = targetParentId ? parseSmartFolderIdNum(targetParentId) : null;
                const siblings = smartFolderNodes
                  .filter((n) => n.parent_id === targetParentId)
                  .sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0));
                const movingNodes = siblings.filter((s) => movingSet.has(s.id));
                const without = siblings.filter((s) => !movingSet.has(s.id));
                const targetIdx = without.findIndex((s) => s.id === dropTargetId);
                const insertAt = dropPosition === 'after' ? targetIdx + 1 : targetIdx;
                without.splice(insertAt, 0, ...movingNodes);
                const moves: [number, number][] = without.map((s, i) => [parseSmartFolderIdNum(s.id)!, i]);
                void smartFoldersController.move(draggedId, parentSmartId, moves);
              }
            }
          } else {
            // Folder drag-drop — multi-select aware
            const draggedFolderId = parseFolderId(drag.nodeId);
            const targetFolderId = parseFolderId(dropTargetId);
            if (draggedFolderId != null && targetFolderId != null) {
              const targetNode = folderNodes.find((n) => n.id === dropTargetId);
              const rawFolderIds = sidebarSelection.has(drag.nodeId) && sidebarSelection.size > 1
                ? [...sidebarSelection].filter((id) => id.startsWith('folder:'))
                : [drag.nodeId];
              const movingIds = deduplicateParentChild(rawFolderIds, folderNodes);
              const movingSet = new Set(movingIds);

              if (dropPosition === 'inside') {
                for (const id of movingIds) {
                  const fid = parseFolderId(id);
                  if (fid != null) void foldersController.move(fid, targetFolderId, []);
                }
              } else {
                const targetParentId = targetNode?.parent_id ?? null;
                const parentFolderId = targetParentId ? parseFolderId(targetParentId) : null;
                const siblings = folderNodes
                  .filter((n) => n.parent_id === targetParentId)
                  .sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0));
                const movingNodes = siblings.filter((s) => movingSet.has(s.id));
                const without = siblings.filter((s) => !movingSet.has(s.id));
                const targetIdx = without.findIndex((s) => s.id === dropTargetId);
                const insertAt = dropPosition === 'after' ? targetIdx + 1 : targetIdx;
                without.splice(insertAt, 0, ...movingNodes);
                const moves: [number, number][] = without.map((s, i) => [parseFolderId(s.id)!, i]);
                void foldersController.move(draggedFolderId, parentFolderId, moves);
              }
            }
          }
        }
      }
      setFolderDragState(null);
    };

    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    return () => {
      document.documentElement.removeAttribute('data-sidebar-drag-active');
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };
  }, [folderDragState, folderNodes, smartFolderNodes, sidebarSelection]);

  const folderRename = useInlineRename({
    onCommit: (id, name) => {
      const fid = parseFolderId(id);
      if (fid != null) { foldersController.rename(fid, name); return; }
      const node = smartFolderNodes.find((entry) => entry.id === id);
      const payload = node ? buildSmartFolderPayloadFromNode(node, { name }) : null;
      const smartFolderId = node ? parseSmartFolderIdNum(node.id) : null;
      if (smartFolderId != null && payload) {
        void smartFoldersController.update(smartFolderId, payload);
      }
    },
  });

  // Pending rename: when a new folder is created, we queue its ID here
  // and start inline rename once the node appears in the tree.
  useEffect(() => { sidebarController.ensureLoaded(); }, []);

  // Trigger pending rename when folder nodes update
  useEffect(() => {
    const pendingId = pendingRenameNodeId;
    if (!pendingId) return;
    const node = [...folderNodes, ...smartFolderNodes].find((n) => n.id === pendingId);
    if (node) {
      setPendingRenameNodeId(null);
      folderRename.startRename(node.id, node.name);
    }
  }, [folderNodes, folderRename, pendingRenameNodeId, setPendingRenameNodeId, smartFolderNodes]);

  const createFolderAndRename = useCallback(async (parentId?: number | null) => {
    const nodeId = await foldersController.create('New Folder', parentId);
    if (nodeId) {
      setPendingRenameNodeId(nodeId);
    }
  }, [setPendingRenameNodeId]);

  const createSmartFolderGroupAndRename = useCallback(async (parentId: number | null = null) => {
    const nodeId = await smartFoldersController.createGroup('Untitled', parentId);
    setPendingRenameNodeId(nodeId);
  }, [setPendingRenameNodeId]);

  useShortcutScope((event) => {
    const newFolder = getShortcut('file.newFolder');
    const newSubfolder = getShortcut('file.newSubfolder');
    const autoTags = getShortcut('folder.autoTags');
    if (newFolder && matchesShortcutDef(event, newFolder)) {
      event.preventDefault();
      void createFolderAndRename();
      return;
    }
    const selectedFolderIds = [...sidebarSelection]
      .map(parseFolderId)
      .filter((id): id is number => id != null);
    const activeFolderId = parseFolderId(activeNodeId ?? '');
    const targetFolderIds = selectedFolderIds.length > 0
      ? selectedFolderIds
      : activeFolderId == null ? [] : [activeFolderId];
    if (newSubfolder && matchesShortcutDef(event, newSubfolder) && targetFolderIds.length === 1) {
      event.preventDefault();
      void createFolderAndRename(targetFolderIds[0]);
      return;
    }
    if (autoTags && matchesShortcutDef(event, autoTags) && targetFolderIds.length > 0) {
      event.preventDefault();
      void openFolderAutoTagsEditor(targetFolderIds);
    }
  }, { priority: 20 });

  const openSmartFolderModal = useCallback((mode: 'create' | 'edit', initial?: {
    id?: number;
    name?: string;
    parent_id?: number | null;
    icon?: string | null;
    color?: string | null;
    notes?: string | null;
    predicate?: SmartFolderPredicate;
    display_order?: number | null;
  }, editor: 'all' | 'details' | 'rules' = mode === 'create' ? 'all' : 'details') => {
    setSmartFolderModal({ open: true, mode, editor, initial });
  }, [setSmartFolderModal]);

  const navigate = useCallback((id: string) => {
    navigateToNode(id);
  }, []);

  const setNodesExpanded = useCallback((nodeIds: readonly string[], expanded: boolean) => {
    for (const id of nodeIds) {
      if (collapsed.has(id) === expanded) toggleCollapse(id);
    }
  }, [collapsed, toggleCollapse]);

  const expandableNodeIds = useCallback((treeNodes: SidebarNodeDto[]) => {
    const parentIds = new Set(treeNodes.map((node) => node.parent_id).filter(Boolean));
    return treeNodes.filter((node) => parentIds.has(node.id)).map((node) => node.id);
  }, []);

  const orderedSystemNodes = useMemo(() => {
    const byId = new Map(systemNodes.map((n) => [n.id, n]));
    const result: SidebarNodeDto[] = [];
    for (const id of SYSTEM_SCOPE_ORDER) {
      const node = byId.get(id);
      if (node) result.push(node);
    }
    return result;
  }, [systemNodes]);
  const treeFilterActive = treeFilter.trim().length > 0;
  const visibleFolderNodes = useMemo(
    () => filterSidebarTree(folderNodes, treeFilter),
    [folderNodes, treeFilter],
  );
  const visibleSmartFolderNodes = useMemo(
    () => filterSidebarTree(smartFolderNodes, treeFilter),
    [smartFolderNodes, treeFilter],
  );
  const treeCollapsed = treeFilterActive ? EXPAND_FILTERED_TREE : collapsed;
  const folderList = useMemo(
    () => buildTreeRenderList(visibleFolderNodes, 'section:folders', treeCollapsed),
    [visibleFolderNodes, treeCollapsed],
  );
  const smartList = useMemo(
    () => buildTreeRenderList(visibleSmartFolderNodes, 'section:smart_folders', treeCollapsed),
    [visibleSmartFolderNodes, treeCollapsed],
  );
  const quickAccessNodes = useMemo(() => {
    const byId = new Map([...folderNodes, ...smartFolderNodes].map((node) => [node.id, node]));
    return quickAccessIds.flatMap((id) => {
      const node = byId.get(id);
      return node ? [node] : [];
    });
  }, [folderNodes, quickAccessIds, smartFolderNodes]);

  /** Multi-select-aware click handler for folder / smart folder rows. */
  const handleRowClick = useCallback((id: string, e: React.MouseEvent) => {
    const isMod = e.metaKey || e.ctrlKey;
    const isShift = e.shiftKey;

    if (isMod) {
      setSidebarSelection((prev) => nextSidebarSelection(prev, id, 'toggle'));
      lastClickedRef.current = id;
      return; // don't navigate
    }

    if (isShift && lastClickedRef.current) {
      // Range select within same section
      const sameSection = id.startsWith('folder:') && lastClickedRef.current.startsWith('folder:')
        ? folderList
        : id.startsWith('smart:') && lastClickedRef.current.startsWith('smart:')
          ? smartList
          : null;
      if (sameSection) {
        const ids = sameSection.map((entry) => entry.node.id);
        const fromIdx = ids.indexOf(lastClickedRef.current);
        const toIdx = ids.indexOf(id);
        if (fromIdx >= 0 && toIdx >= 0) {
          const lo = Math.min(fromIdx, toIdx);
          const hi = Math.max(fromIdx, toIdx);
          setSidebarSelection((prev) => nextSidebarSelection(prev, id, 'range', ids.slice(lo, hi + 1)));
          return; // don't navigate
        }
      }
    }

    // The active row is also the first selection anchor for Cmd/Ctrl and Shift selection.
    setSidebarSelection((prev) => nextSidebarSelection(prev, id, 'replace'));
    lastClickedRef.current = id;
    navigate(id);
  }, [navigate, folderList, smartList]);

  // ── Context menu builders ──────────────────────────────────────

  const openFolderMenu = useCallback((e: React.MouseEvent, node: SidebarNodeDto) => {
    setContextMenuNodeId(node.id);
    const folderId = parseFolderId(node.id);
    if (folderId == null) return;
    const isExpanded = !collapsed.has(node.id);
    const hasChildren = folderList.some(({ node: n }) => n.parent_id === node.id);
    const allExpandableIds = expandableNodeIds(folderNodes);
    const anyTreeExpanded = allExpandableIds.some((id) => !collapsed.has(id));
    const entries: MenuEntry[] = [
      { label: 'New Folder', icon: <IconFolderPlus size={14} />, shortcut: kbd('file.newFolder'), action: () => { void createFolderAndRename(); } },
      { label: 'New Subfolder', icon: <IconNewSubfolder size={14} />, shortcut: kbd('file.newSubfolder'), action: () => { void createFolderAndRename(folderId); } },
      { separator: true },
      { label: quickAccessIds.includes(node.id) ? 'Remove from Quick Access' : 'Add to Quick Access', icon: quickAccessIds.includes(node.id) ? <IconStarOff size={14} /> : <IconStar size={14} />, action: () => {
        void (quickAccessIds.includes(node.id) ? removeQuickAccess(node.id) : addQuickAccess(node.id));
      } },
      { label: 'Rename', icon: <IconRename size={14} />, shortcut: kbd('edit.rename'), action: () => folderRename.startRename(node.id, node.name) },
      { label: 'Move to...', icon: <IconFolderOpen size={14} />, action: () => {
        const currentParentId = parseFolderId(node.parent_id ?? '');
        setFolderPortal({
          open: true,
          anchor: { x: e.clientX, y: e.clientY },
          selectedFolderIds: currentParentId == null ? [] : [currentParentId],
          availableFolderIds: availableFolderMoveTargets(folderNodes, folderId),
          onApplyFolderParent: (parentId) => { void foldersController.move(folderId, parentId, []); },
        });
      } },
      { label: 'Duplicate', icon: <IconCopy size={14} />, action: () => {
        void foldersController.duplicate(folderId).then((duplicateNodeId) => {
          const duplicateName = `${node.name} copy`;
          folderRename.startRename(duplicateNodeId, duplicateName);
        });
      } },
      { label: 'Set Auto Tags...', icon: <IconAutoTags size={14} />, shortcut: kbd('folder.autoTags'), action: () => {
        void openFolderAutoTagsEditor([folderId]);
      } },
      { separator: true },
      { label: 'Import Folder Here...', icon: <IconFolderPlus size={14} />, action: () => {
        void chooseAndImportFolder({ kind: 'folder', folder_id: folderId });
      } },
      { label: 'Attach Watched Folder...', icon: <IconWatchFolder size={14} />, action: () => {
        store.set(folderWatchModalAtom, { open: true, folderId, initial: {} });
      } },
      ...((node.meta as Record<string, unknown> | null)?.watch_enabled ? [{
        label: 'Remove Watched Folder', icon: <IconWatchFolder size={14} />,
        action: () => {
          store.set(confirmModalAtom, {
            open: true, title: 'Remove Watch', danger: true, confirmLabel: 'Remove',
            message: `Stop watching the folder for "${node.name}"?`,
            onConfirm: () => { void foldersController.clearWatchConfig(folderId); },
          });
        },
      } as MenuEntry] : []),
      { separator: true },
      { submenu: true, label: 'Sort Folders', icon: <IconSort size={14} />, children: [
        { label: 'This Level A–Z', action: () => { void foldersController.sortTree(folderId, false, false); } },
        { label: 'This Level Z–A', action: () => { void foldersController.sortTree(folderId, true, false); } },
        { label: 'This Level and Descendants A–Z', action: () => { void foldersController.sortTree(folderId, false, true); } },
        { label: 'This Level and Descendants Z–A', action: () => { void foldersController.sortTree(folderId, true, true); } },
      ] },
      { label: 'Sort Contents by Name', icon: <IconSort size={14} />, action: () => { void foldersController.sortByName(folderId); } },
      { label: isExpanded ? 'Collapse Folder' : 'Expand Folder', icon: isExpanded ? <IconCollapse size={14} /> : <IconExpand size={14} />,
        action: () => { if (hasChildren) toggleCollapse(node.id); },
        disabled: !hasChildren },
      {
        label: anyTreeExpanded ? 'Collapse All Folders' : 'Expand All Folders',
        icon: anyTreeExpanded ? <IconCollapseAll size={14} /> : <IconExpandAll size={14} />,
        action: () => setNodesExpanded(allExpandableIds, !anyTreeExpanded),
        disabled: allExpandableIds.length === 0,
      },
      { separator: true },
      { submenu: true, label: 'Change Icon', icon: <IconChangeIcon size={14} />, children: [
        { custom: true, key: 'folder-icon', render: () => (
          <IconPicker compact value={node.icon ?? null} onChange={(icon) => { void foldersController.applyIcon(folderId, icon); }} />
        ) },
      ] },
      { custom: true, key: 'folder-color', render: () => (
        <ColorPicker value={node.color ?? null} onChange={(hex) => foldersController.applyColor(folderId, hex)} />
      ) },
      { separator: true },
      scopeExportEntry(queryTarget({ kind: 'folder', folder_id: folderId }), node.count ?? 0),
      { separator: true },
      { label: 'Delete', icon: <IconTrash size={14} />, danger: true, action: () => {
        store.set(confirmModalAtom, {
          open: true, title: 'Delete Folder', danger: true, confirmLabel: 'Delete',
          message: singleFolderDeletionMessage(node.name),
          onConfirm: () => foldersController.delete(folderId),
        });
      } },
    ];
    contextMenu.open(e, entries);
  }, [contextMenu, folderRename, collapsed, toggleCollapse, folderNodes, setFolderPortal, expandableNodeIds, setNodesExpanded, quickAccessIds]);

  const openSmartFolderMenu = useCallback((e: React.MouseEvent, node: SidebarNodeDto) => {
    setContextMenuNodeId(node.id);
    const sfId = parseSmartFolderId(node.id);
    const sfIdNum = parseSmartFolderIdNum(node.id);
    if (sfId == null) return;
    const currentPayload = buildSmartFolderPayloadFromNode(node);
    const isGroup = isSmartFolderGroup(node);
    const isExpanded = !collapsed.has(node.id);
    const hasChildren = smartList.some(({ node: child }) => child.parent_id === node.id);
    const allExpandableIds = expandableNodeIds(smartFolderNodes);
    const anyTreeExpanded = allExpandableIds.some((id) => !collapsed.has(id));
    const moveTargets = availableTreeMoveTargetIds(smartFolderNodes, [node.id]);
    const entries: MenuEntry[] = [
      ...(!isGroup ? [{
        label: 'Edit Smart Folder...',
        icon: <IconFolderOpen size={14} />,
        action: () => openSmartFolderModal('edit', smartFolderInitialFromNode(node), 'details'),
      } satisfies MenuEntry, {
        label: 'Edit Rules...',
        icon: <IconAdjustments size={14} />,
        action: () => openSmartFolderModal('edit', smartFolderInitialFromNode(node), 'rules'),
      } satisfies MenuEntry] : []),
      {
        label: 'New Child Smart Folder',
        icon: <IconFolderPlus size={14} />,
        action: () => openSmartFolderModal('create', {
          name: 'New Smart Folder',
          parent_id: sfIdNum,
          predicate: { groups: [] },
        }),
      },
      {
        label: 'New Child Smart Folder Group',
        icon: <IconLayoutGrid size={14} />,
        action: () => { if (sfIdNum != null) void createSmartFolderGroupAndRename(sfIdNum); },
      },
      { separator: true },
      ...(!isGroup ? [{ label: quickAccessIds.includes(node.id) ? 'Remove from Quick Access' : 'Add to Quick Access', icon: quickAccessIds.includes(node.id) ? <IconStarOff size={14} /> : <IconStar size={14} />, action: () => {
        void (quickAccessIds.includes(node.id) ? removeQuickAccess(node.id) : addQuickAccess(node.id));
      } } satisfies MenuEntry] : []),
      { label: 'Rename', icon: <IconRename size={14} />, action: () => folderRename.startRename(node.id, node.name) },
      { submenu: true, label: 'Move to...', icon: <IconFolderOpen size={14} />, children: [
        { label: 'Top Level', action: () => { if (sfIdNum != null) void smartFoldersController.move(sfIdNum, null, []); } },
        ...moveTargets.map((targetNodeId) => {
          const target = smartFolderNodes.find((candidate) => candidate.id === targetNodeId)!;
          return {
            label: target.name,
            action: () => {
              const targetId = parseSmartFolderIdNum(targetNodeId);
              if (sfIdNum != null && targetId != null) void smartFoldersController.move(sfIdNum, targetId, []);
            },
          } as MenuEntry;
        }),
      ] },
      { label: 'Duplicate', icon: <IconCopy size={14} />, action: () => {
        void (async () => {
          const name = `${node.name} copy`;
          const duplicateId = await smartFoldersController.create({
            ...currentPayload,
            smart_folder_id: 0,
            name,
          });
          folderRename.startRename(duplicateId, name);
        })();
      } },
      { separator: true },
      ...(!isGroup ? [{
        label: 'Refresh Results', icon: <IconRefresh size={14} />, action: () => {
          if (sfIdNum == null) return;
          void smartFoldersController.refresh(sfIdNum).catch((reason) => showErrorNotification({
            title: 'Could not refresh smart folder',
            message: reason instanceof Error ? reason.message : String(reason),
          }));
        },
      } satisfies MenuEntry, {
        label: 'Sort Results by Name', icon: <IconSort size={14} />, action: () => {
          if (sfIdNum != null) void smartFoldersController.update(sfIdNum, {
            ...currentPayload,
            sort_field: 'name',
            sort_order: 'ascending',
          });
        },
      } satisfies MenuEntry] : []),
      {
        label: isExpanded ? 'Collapse Smart Folder' : 'Expand Smart Folder',
        icon: isExpanded ? <IconCollapse size={14} /> : <IconExpand size={14} />,
        action: () => { if (hasChildren) toggleCollapse(node.id); },
        disabled: !hasChildren,
      },
      {
        label: anyTreeExpanded ? 'Collapse All Smart Folders' : 'Expand All Smart Folders',
        icon: anyTreeExpanded ? <IconCollapseAll size={14} /> : <IconExpandAll size={14} />,
        action: () => setNodesExpanded(allExpandableIds, !anyTreeExpanded),
        disabled: allExpandableIds.length === 0,
      },
      { separator: true },
      { submenu: true, label: 'Change Icon', icon: <IconChangeIcon size={14} />, children: [
        { custom: true, key: 'sf-icon', render: () => (
          <IconPicker compact value={node.icon ?? null} onChange={(icon) => {
            if (sfIdNum != null) {
              void smartFoldersController.update(sfIdNum, { ...currentPayload, icon });
            }
          }} />
        ) },
      ] },
      { custom: true, key: 'sf-color', render: () => (
        <ColorPicker value={node.color ?? null} onChange={(hex) => {
          if (sfIdNum != null) {
            void smartFoldersController.update(sfIdNum, { ...currentPayload, color: hex });
          }
        }} />
      ) },
      { separator: true },
      ...(sfIdNum == null || isGroup ? [] : [scopeExportEntry(
        queryTarget({ kind: 'smart_folder', smart_folder_id: sfIdNum }),
        node.count ?? 0,
      )]),
      { separator: true },
      { label: 'Delete', icon: <IconTrash size={14} />, danger: true, action: () => {
        store.set(confirmModalAtom, {
          open: true, title: 'Delete Smart Folder', danger: true, confirmLabel: 'Delete',
          message: isGroup
            ? `Delete "${node.name}" and its child smart folders? This does not delete any media.`
            : `Delete "${node.name}"? This only removes the smart folder, not its contents.`,
          onConfirm: () => smartFoldersController.delete(sfId),
        });
      } },
    ];
    contextMenu.open(e, entries);
  }, [collapsed, contextMenu, createSmartFolderGroupAndRename, expandableNodeIds, folderRename, openSmartFolderModal, setNodesExpanded, smartFolderNodes, smartList, toggleCollapse, quickAccessIds]);

  const persistSystemVisibility = useCallback((
    setting: keyof Pick<AppSettings,
      | 'showSidebarInbox'
      | 'showSidebarRecentlyViewed'
      | 'showSidebarUncategorized'
      | 'showSidebarUntagged'
      | 'showSidebarTagManager'
      | 'showSidebarRandom'
      | 'showSidebarSubscriptions'
      | 'showSidebarDuplicates'>,
    nodeId: string,
  ) => {
    const visible = !sidebarPreferences.visibleSystemNodes.has(nodeId);
    setSidebarPreferences((current) => {
      const visibleSystemNodes = new Set(current.visibleSystemNodes);
      if (visible) visibleSystemNodes.add(nodeId);
      else visibleSystemNodes.delete(nodeId);
      return { ...current, visibleSystemNodes };
    });
    void settingsController.patchSettings({ [setting]: visible });
  }, [setSidebarPreferences, sidebarPreferences.visibleSystemNodes]);

  const persistSectionVisibility = useCallback((
    setting: keyof Pick<AppSettings, 'showSidebarQuickAccess' | 'showSidebarFolders' | 'showSidebarSmartFolders'>,
    preference: 'showQuickAccess' | 'showFolders' | 'showSmartFolders',
  ) => {
    const visible = !sidebarPreferences[preference];
    setSidebarPreferences((current) => ({ ...current, [preference]: visible }));
    void settingsController.patchSettings({ [setting]: visible });
  }, [setSidebarPreferences, sidebarPreferences]);

  const sidebarVisibilityEntries = useMemo(() => {
    const systemEntry = (
      label: string,
      icon: ReactNode,
      nodeId: string,
      setting: Parameters<typeof persistSystemVisibility>[0],
    ): MenuEntry => ({
      label,
      icon,
      checked: sidebarPreferences.visibleSystemNodes.has(nodeId),
      keepOpen: true,
      action: () => persistSystemVisibility(setting, nodeId),
    });

    return [
      { label: 'All', icon: <IconPhoto size={14} />, checked: true, disabled: true, action: () => {} },
      systemEntry('Inbox', <IconInbox size={14} />, 'system:inbox', 'showSidebarInbox'),
      systemEntry('Recently Viewed', <IconClock size={14} />, 'system:recent_viewed', 'showSidebarRecentlyViewed'),
      systemEntry('Uncategorized', <IconFolderQuestionCustom size={14} />, 'system:uncategorized', 'showSidebarUncategorized'),
      systemEntry('Untagged', <IconBookmarkQuestionCustom size={14} />, 'system:untagged', 'showSidebarUntagged'),
      systemEntry('Tag Manager', <IconBookmark size={14} />, 'system:tag_manager', 'showSidebarTagManager'),
      systemEntry('Random', <IconArrowsShuffle size={14} />, 'system:random', 'showSidebarRandom'),
      systemEntry('Subscriptions', <IconDownload size={14} />, 'system:subscriptions', 'showSidebarSubscriptions'),
      systemEntry('Duplicates', <IconCopy size={14} />, 'system:duplicates', 'showSidebarDuplicates'),
      { label: 'Trash', icon: <IconTrash size={14} />, checked: true, disabled: true, action: () => {} },
      { separator: true },
      {
        label: 'Quick Access', icon: <IconStar size={14} />, checked: sidebarPreferences.showQuickAccess,
        keepOpen: true,
        action: () => persistSectionVisibility('showSidebarQuickAccess', 'showQuickAccess'),
      },
      {
        label: 'Smart Folders', icon: <IconBookmark size={14} />, checked: sidebarPreferences.showSmartFolders,
        keepOpen: true,
        action: () => persistSectionVisibility('showSidebarSmartFolders', 'showSmartFolders'),
      },
      {
        label: 'Folders', icon: <IconFolder size={14} />, checked: sidebarPreferences.showFolders,
        keepOpen: true,
        action: () => persistSectionVisibility('showSidebarFolders', 'showFolders'),
      },
    ] satisfies MenuEntry[];
  }, [persistSectionVisibility, persistSystemVisibility, sidebarPreferences]);

  const openSidebarVisibilityMenu = useCallback((event: React.MouseEvent) => {
    setSidebarVisibilityMenuOpen(true);
    contextMenu.open(event, sidebarVisibilityEntries, { showSearch: false });
  }, [contextMenu, sidebarVisibilityEntries]);

  const openSystemMenu = useCallback((event: React.MouseEvent, node: SidebarNodeDto) => {
    if (node.id === 'system:recent_viewed') {
      contextMenu.open(event, [{
        label: 'Clear Recently Viewed',
        icon: <IconTrash size={14} />,
        disabled: (node.count ?? 0) === 0,
        action: () => {
          void clearRecentViews()
            .then(() => announceUndoableMutation('items.clear_recent_views'));
        },
      }], { showSearch: false });
      return;
    }
    if (node.id !== 'system:trash') {
      openSidebarVisibilityMenu(event);
      return;
    }
    const target = queryTarget({ kind: 'trash' });
    const disabled = (node.count ?? 0) === 0;
    contextMenu.open(event, [
      {
        label: 'Empty Trash',
        icon: <IconTrash size={14} />,
        disabled,
        danger: true,
        keywords: 'delete remove trash permanently',
        action: () => {
          store.set(confirmModalAtom, {
            open: true,
            title: 'Empty Trash',
            message: 'Permanently delete every item in Trash? This cannot be undone.',
            confirmLabel: 'Delete All',
            danger: true,
            onConfirm: () => entityMutations.permanentlyDeleteTarget(target),
          });
        },
      },
      {
        label: 'Restore All',
        icon: <IconDownload size={14} />,
        disabled,
        keywords: 'restore recover trash',
        action: () => { void entityMutations.setTargetLifecycle(target, 'active'); },
      },
    ], { showSearch: false });
  }, [contextMenu, openSidebarVisibilityMenu]);

  // ── Bulk context menu ──────────────────────────────────────────

  const openBulkMenu = useCallback((e: React.MouseEvent) => {
    const sel = sidebarSelection;
    const folderIds = [...sel].filter((id) => id.startsWith('folder:'));
    const smartIds = [...sel].filter((id) => id.startsWith('smart:'));
    const allFolders = smartIds.length === 0;
    const allSmart = folderIds.length === 0;

    const entries: MenuEntry[] = [];
    const selectedIds = [...sel].filter((id) => id.startsWith('folder:') || id.startsWith('smart:'));
    const selectedQuickIds = selectedIds.filter((id) => quickAccessIds.includes(id));
    const selectedNonQuickIds = selectedIds.filter((id) => !quickAccessIds.includes(id));
    if (selectedNonQuickIds.length > 0) {
      entries.push({
        label: 'Add to Quick Access', icon: <IconStar size={14} />,
        action: () => { void reorderQuickAccess([...quickAccessIds, ...selectedNonQuickIds]); },
      });
    }
    if (selectedQuickIds.length > 0) {
      entries.push({
        label: 'Remove from Quick Access', icon: <IconStarOff size={14} />,
        action: () => { void reorderQuickAccess(quickAccessIds.filter((id) => !selectedQuickIds.includes(id))); },
      });
    }
    entries.push({
      label: `Duplicate ${selectedIds.length} item${selectedIds.length === 1 ? '' : 's'}`,
      icon: <IconCopy size={14} />,
      action: () => {
        void Promise.all([
          ...folderIds.map((id) => {
            const folderId = parseFolderId(id);
            return folderId == null ? Promise.resolve() : foldersController.duplicate(folderId);
          }),
          ...smartIds.map((id) => {
            const node = smartFolderNodes.find((candidate) => candidate.id === id);
            if (!node) return Promise.resolve();
            return smartFoldersController.create({
              ...buildSmartFolderPayloadFromNode(node),
              smart_folder_id: 0,
              name: `${node.name} copy`,
            });
          }),
        ]);
      },
    });
    if (allFolders) {
      const autoTagFolderIds = folderIds.map(parseFolderId).filter((id): id is number => id != null);
      entries.push({
        label: 'Set Auto Tags...', icon: <IconAutoTags size={14} />, shortcut: kbd('folder.autoTags'),
        action: () => { void openFolderAutoTagsEditor(autoTagFolderIds); },
      });
    }
    entries.push({ separator: true });

    if (allFolders) {
      const movingIds = deduplicateParentChild(folderIds, folderNodes);
      const availableIds = new Set(availableTreeMoveTargetIds(folderNodes, movingIds));
      const movingFolderIds = movingIds.map(parseFolderId).filter((id): id is number => id != null);
      const parentIds = movingIds.map((id) => folderNodes.find((node) => node.id === id)?.parent_id ?? null);
      const sharedParent = parentIds.every((parentId) => parentId === parentIds[0])
        ? parseFolderId(parentIds[0] ?? '')
        : null;
      entries.push({
        label: 'Move to...', icon: <IconFolderOpen size={14} />, action: () => {
          setFolderPortal({
            open: true,
            anchor: { x: e.clientX, y: e.clientY },
            selectedFolderIds: sharedParent == null ? [] : [sharedParent],
            availableFolderIds: folderNodes
              .map((candidate) => parseFolderId(candidate.id))
              .filter((id): id is number => id != null && availableIds.has(`folder:${id}`)),
            onApplyFolderParent: (parentId) => {
              void Promise.all(movingFolderIds.map((folderId) => foldersController.move(folderId, parentId, [])));
            },
          });
        },
      });
      entries.push({
        label: 'Sort Contents by Name', icon: <IconSort size={14} />,
        action: () => { void Promise.all(movingFolderIds.map((folderId) => foldersController.sortByName(folderId))); },
      });
    } else if (allSmart) {
      const movingIds = deduplicateParentChild(smartIds, smartFolderNodes);
      const availableIds = availableTreeMoveTargetIds(smartFolderNodes, movingIds);
      const movingSmartIds = movingIds.map(parseSmartFolderIdNum).filter((id): id is number => id != null);
      entries.push({ submenu: true, label: 'Move to...', icon: <IconFolderOpen size={14} />, children: [
        {
          label: 'Top Level',
          action: () => { void Promise.all(movingSmartIds.map((id) => smartFoldersController.move(id, null, []))); },
        },
        ...availableIds.map((targetNodeId) => {
          const target = smartFolderNodes.find((candidate) => candidate.id === targetNodeId)!;
          return {
            label: target.name,
            action: () => {
              const parentId = parseSmartFolderIdNum(targetNodeId);
              if (parentId != null) void Promise.all(movingSmartIds.map((id) => smartFoldersController.move(id, parentId, [])));
            },
          } as MenuEntry;
        }),
      ] });
      entries.push({
        label: 'Sort Results by Name', icon: <IconSort size={14} />,
        action: () => {
          void Promise.all(movingIds.map((id) => {
            const node = smartFolderNodes.find((candidate) => candidate.id === id);
            const smartFolderId = parseSmartFolderIdNum(id);
            if (!node || smartFolderId == null) return Promise.resolve();
            return smartFoldersController.update(smartFolderId, {
              ...buildSmartFolderPayloadFromNode(node),
              sort_field: 'name',
              sort_order: 'ascending',
            });
          }));
        },
      });
    }

    if (allFolders || allSmart) {
      const treeNodes = allFolders ? folderNodes : smartFolderNodes;
      const selectedIds = allFolders ? folderIds : smartIds;
      const expandable = new Set(expandableNodeIds(treeNodes));
      const expandableSelected = selectedIds.filter((id) => expandable.has(id));
      const hasCollapsed = expandableSelected.some((id) => collapsed.has(id));
      const hasExpanded = expandableSelected.some((id) => !collapsed.has(id));
      entries.push({
        label: 'Expand Selected', icon: <IconExpand size={14} />,
        action: () => setNodesExpanded(expandableSelected, true),
        disabled: !hasCollapsed,
      });
      entries.push({
        label: 'Collapse Selected', icon: <IconCollapse size={14} />,
        action: () => setNodesExpanded(expandableSelected, false),
        disabled: !hasExpanded,
      });
      entries.push({ separator: true });
    }

    // Change Color — only when all same type
    if (allFolders || allSmart) {
      entries.push({
        custom: true, key: 'bulk-color', render: () => (
          <ColorPicker value={null} onChange={(hex) => {
            for (const id of folderIds) {
              const fid = parseFolderId(id);
              if (fid != null) void foldersController.applyColor(fid, hex);
            }
            for (const id of smartIds) {
              const sfIdNum = parseSmartFolderIdNum(id);
              const node = smartFolderNodes.find((n) => n.id === id);
              if (sfIdNum != null && node) {
                const payload = buildSmartFolderPayloadFromNode(node, { color: hex });
                void smartFoldersController.update(sfIdNum, payload);
              }
            }
          }} />
        ),
      });
      entries.push({ separator: true });
    }

    // Delete — always available
    const totalCount = sel.size;
    entries.push({
      label: `Delete ${totalCount} items`, icon: <IconTrash size={14} />, danger: true,
      action: () => {
        store.set(confirmModalAtom, {
          open: true, title: 'Delete Selected', danger: true, confirmLabel: 'Delete',
          message: bulkFolderDeletionMessage(totalCount),
          onConfirm: () => {
            const folderDeleteIds = folderIds
              .map(parseFolderId)
              .filter((id): id is number => id != null);
            const deletes: Promise<unknown>[] = [];
            if (folderDeleteIds.length > 0) deletes.push(foldersController.deleteMany(folderDeleteIds));
            for (const id of smartIds) {
              const sfId = parseSmartFolderId(id);
              if (sfId != null) deletes.push(smartFoldersController.delete(sfId));
            }
            void Promise.all(deletes).then(() => setSidebarSelection(new Set())).catch(() => {});
          },
        });
      },
    });

    setContextMenuNodeId(null);
    contextMenu.open(e, entries);
  }, [collapsed, contextMenu, expandableNodeIds, folderNodes, setFolderPortal, setNodesExpanded, sidebarSelection, smartFolderNodes, quickAccessIds]);

  /** Unified context menu handler — dispatches to bulk or single. */
  const handleFolderContextMenu = useCallback((e: React.MouseEvent, node: SidebarNodeDto) => {
    if (sidebarSelection.size > 1 && sidebarSelection.has(node.id)) {
      openBulkMenu(e);
    } else {
      // Clear selection, show single-item menu
      setSidebarSelection(new Set());
      openFolderMenu(e, node);
    }
  }, [sidebarSelection, openBulkMenu, openFolderMenu]);

  const handleSmartFolderContextMenu = useCallback((e: React.MouseEvent, node: SidebarNodeDto) => {
    if (sidebarSelection.size > 1 && sidebarSelection.has(node.id)) {
      openBulkMenu(e);
    } else {
      setSidebarSelection(new Set());
      openSmartFolderMenu(e, node);
    }
  }, [sidebarSelection, openBulkMenu, openSmartFolderMenu]);

  return (
    <div className={styles.root}>
      <LibrarySwitcherButton />
      <div className={styles.scroll}>
        {loading && nodes.length === 0 && (
          <div className={styles.loadingMessage}>Loading…</div>
        )}

        {/* System scopes — fixed order */}
        {orderedSystemNodes.filter((node) => sidebarPreferences.visibleSystemNodes.has(node.id)).map((node) => {
          const ScopeIcon = SYSTEM_ICONS[node.id];
          const statusDropMap: Record<string, string> = {
            'system:active': '1',
            'system:inbox': '0',
            'system:trash': '2',
          };
          const statusDrop = statusDropMap[node.id];
          return (
            <SidebarRow
              key={node.id}
              icon={ScopeIcon ? <ScopeIcon size={IC} {...FILL} /> : undefined}
              label={LABEL_OVERRIDES[node.id] ?? node.name}
              count={sidebarPreferences.showCounts ? node.count : undefined}
              activityLabel={node.id === 'system:subscriptions' && subscriptionsRunning
                ? 'Subscription running'
                : undefined}
              active={activeNodeId === node.id}
              onClick={() => { if (node.selectable) { setSidebarSelection(new Set()); navigate(node.id); } }}
              onContextMenu={(event) => openSystemMenu(event, node)}
              dropDataAttr={statusDrop ? { key: 'status-drop', value: statusDrop } : undefined}
              dataHelpId={SYSTEM_HELP_IDS[node.id]}
            />
          );
        })}

        {sidebarPreferences.showQuickAccess && quickAccessNodes.length > 0 && (
          <>
            <SidebarRow
              variant="section"
              label="Quick Access"
              count={sidebarPreferences.showCounts ? quickAccessNodes.length : undefined}
              expanded={!collapsed.has('quick_access')}
              onToggle={() => toggleCollapse('quick_access')}
              dataHelpId="sidebar-quick-access"
            />
            {!collapsed.has('quick_access') && quickAccessNodes.map((node) => (
              <SidebarRow
                key={`quick-${node.id}`}
                variant={node.kind === 'folder' ? 'folder' : 'smart_folder'}
                icon={<NodeIcon node={node} expanded={false} />}
                label={node.name}
                count={sidebarPreferences.showCounts ? node.count : undefined}
                active={activeNodeId === node.id}
                contextHighlight={contextMenuNodeId === node.id}
                onClick={(event) => handleRowClick(node.id, event)}
                onContextMenu={(event) => {
                  if (node.kind === 'folder') handleFolderContextMenu(event, node);
                  else handleSmartFolderContextMenu(event, node);
                }}
                dataHelpRegion="sidebar-quick-access"
              />
            ))}
          </>
        )}

        {/* Folders */}
        {sidebarPreferences.showFolders && <SidebarRow
          variant="section"
          label="Folders"
          count={sidebarPreferences.showCounts ? folderNodes.length : undefined}
          expanded={treeFilterActive || !collapsed.has('folders')}
          onToggle={() => { if (!treeFilterActive) toggleCollapse('folders'); }}
          onAdd={() => { void createFolderAndRename(); }}
          onContextMenu={(event) => {
            contextMenu.open(event, [
              {
                label: 'New Folder', icon: <IconFolderPlus size={14} />, shortcut: kbd('file.newFolder'),
                action: () => { void createFolderAndRename(); },
              },
              {
                label: 'Import Folder...', icon: <IconFolderPlus size={14} />,
                action: () => { void chooseAndImportFolder({ kind: 'all' }); },
              },
            ]);
          }}
          addTooltip="New Folder" addShortcutId="file.newFolder"
          dataHelpId="sidebar-folders"
        />}
        {sidebarPreferences.showFolders && (treeFilterActive || !collapsed.has('folders')) && folderList.map(({ node, indent, hasChildren, treeLines, isLastChild }) => (
          <SidebarRow
            key={node.id} variant="folder"
            icon={<NodeIcon node={node} expanded={(treeFilterActive || !collapsed.has(node.id)) && hasChildren} />}
            label={folderRename.renamingId === node.id ? undefined : node.name}
            count={sidebarPreferences.showCounts ? node.count : undefined}

            active={activeNodeId === node.id} indent={indent}
            selected={sidebarSelection.has(node.id)}
            hasChildren={hasChildren} expanded={treeFilterActive || !collapsed.has(node.id)}
            treeLines={treeLines} isLastChild={isLastChild}
            contextHighlight={contextMenuNodeId === node.id}
            dropPosition={folderDragState?.dropTargetId === node.id ? folderDragState.dropPosition ?? undefined : undefined}
            onToggleExpand={treeFilterActive ? undefined : () => toggleCollapse(node.id)}
            onClick={(e) => handleRowClick(node.id, e)}
            onDoubleClick={(e) => {
              e.preventDefault();
              if (sidebarPreferences.doubleClickAction === 'rename') {
                folderRename.startRename(node.id, node.name);
              } else if (hasChildren && !treeFilterActive) {
                toggleCollapse(node.id);
              }
            }}
            onContextMenu={(e) => handleFolderContextMenu(e, node)}
            onPointerDown={(e) => {
              if (e.button !== 0) return;
              folderDragRef.current = { nodeId: node.id, startY: e.clientY };
            }}
            dropDataAttr={{ key: 'folder-drop-id', value: String(parseFolderId(node.id) ?? '') }}
            dataHelpRegion="sidebar-folders"
          >
            {folderRename.renamingId === node.id ? (
              <input
                ref={folderRename.inputRef}
                className={styles.renameInput}
                value={folderRename.renameValue}
                onChange={(e) => folderRename.setRenameValue(e.target.value)}
                onKeyDown={folderRename.handleKeyDown}
                onBlur={folderRename.commitRename}
                onContextMenu={(e) => { e.preventDefault(); folderRename.commitRename(); }}
              />
            ) : undefined}
          </SidebarRow>
        ))}

        {/* Smart Folders */}
        {sidebarPreferences.showSmartFolders && <SidebarRow
          variant="section"
          label="Smart Folders"
          count={sidebarPreferences.showCounts ? smartFolderNodes.length : undefined}
          expanded={treeFilterActive || !collapsed.has('smart_folders')}
          onToggle={() => { if (!treeFilterActive) toggleCollapse('smart_folders'); }}
          onAdd={(event) => {
            const rect = event.currentTarget.getBoundingClientRect();
            contextMenu.openAt({ x: rect.left, y: rect.bottom + 4 }, [
              {
                label: 'New Smart Folder', icon: <IconFilterPlus size={14} />,
                action: () => openSmartFolderModal('create', { name: 'New Smart Folder', predicate: { groups: [] } }),
              },
              {
                label: 'New Smart Folder Group', icon: <IconLayoutGrid size={14} />,
                action: () => { void createSmartFolderGroupAndRename(); },
              },
            ], { showSearch: false });
          }}
          addTooltip="New Smart Folder or Group"
          onContextMenu={(event) => {
            contextMenu.open(event, [
              {
                label: 'New Smart Folder', icon: <IconFilterPlus size={14} />,
                action: () => openSmartFolderModal('create', { name: 'New Smart Folder', predicate: { groups: [] } }),
              },
              {
                label: 'New Smart Folder Group', icon: <IconLayoutGrid size={14} />,
                action: () => { void createSmartFolderGroupAndRename(); },
              },
            ]);
          }}
          dataHelpId="sidebar-smart-folders"
        />}
        {sidebarPreferences.showSmartFolders && (treeFilterActive || !collapsed.has('smart_folders')) && smartList.map(({ node, indent, hasChildren, treeLines, isLastChild }) => (
          <SidebarRow
            key={node.id} variant="smart_folder"
            icon={<NodeIcon node={node} expanded={(treeFilterActive || !collapsed.has(node.id)) && hasChildren} />}
            label={folderRename.renamingId === node.id ? undefined : node.name}
            count={sidebarPreferences.showCounts ? node.count : undefined}

            active={activeNodeId === node.id} indent={indent}
            selected={sidebarSelection.has(node.id)}
            hasChildren={hasChildren} expanded={treeFilterActive || !collapsed.has(node.id)}
            treeLines={treeLines} isLastChild={isLastChild}
            contextHighlight={contextMenuNodeId === node.id}
            dropPosition={folderDragState?.dropTargetId === node.id ? folderDragState.dropPosition ?? undefined : undefined}
            onToggleExpand={treeFilterActive ? undefined : () => toggleCollapse(node.id)}
            onClick={(e) => {
              if (isSmartFolderGroup(node)) {
                if (hasChildren && !treeFilterActive) toggleCollapse(node.id);
                return;
              }
              handleRowClick(node.id, e);
            }}
            onDoubleClick={(e) => {
              e.preventDefault();
              if (sidebarPreferences.doubleClickAction === 'rename') {
                folderRename.startRename(node.id, node.name);
              } else if (hasChildren && !treeFilterActive) {
                toggleCollapse(node.id);
              }
            }}
            onContextMenu={(e) => handleSmartFolderContextMenu(e, node)}
            onPointerDown={(e) => {
              if (e.button !== 0) return;
              folderDragRef.current = { nodeId: node.id, startY: e.clientY };
            }}
            dropDataAttr={{ key: 'smart-drop-id', value: String(parseSmartFolderIdNum(node.id) ?? '') }}
            dataHelpRegion="sidebar-smart-folders"
          >
            {folderRename.renamingId === node.id ? (
              <input
                ref={folderRename.inputRef}
                className={styles.renameInput}
                value={folderRename.renameValue}
                onChange={(e) => folderRename.setRenameValue(e.target.value)}
                onKeyDown={folderRename.handleKeyDown}
                onBlur={folderRename.commitRename}
                onContextMenu={(e) => { e.preventDefault(); folderRename.commitRename(); }}
              />
            ) : undefined}
          </SidebarRow>
        ))}

        {treeFilterActive && ((sidebarPreferences.showFolders && folderList.length === 0) || !sidebarPreferences.showFolders) && ((sidebarPreferences.showSmartFolders && smartList.length === 0) || !sidebarPreferences.showSmartFolders) && (
          <div className={styles.noFilterResults}>No matching folders</div>
        )}
      </div>

      {(sidebarPreferences.showFolders || sidebarPreferences.showSmartFolders) && <div className={styles.treeFilter} data-help-id="sidebar-filter">
        <div className={styles.treeFilterField}>
          <ToolbarFilterIcon className={styles.treeFilterIcon} size={16} />
          <input
            className={`${styles.treeFilterInput}${treeFilter ? ` ${styles.treeFilterInputWithClear}` : ''}`}
            value={treeFilter}
            onChange={(event) => setTreeFilter(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === 'Escape' && treeFilter) {
                event.preventDefault();
                setTreeFilter('');
              }
            }}
            placeholder="Filter"
            aria-label="Filter folders and smart folders"
          />
          {treeFilter && (
            <button className={styles.clearTreeFilter} type="button" onClick={() => setTreeFilter('')} aria-label="Clear folder filter">
              <IconX size={12} />
            </button>
          )}
        </div>
      </div>}

      {/* Context menu portal */}
      {contextMenu.state && (
        <ContextMenu
          entries={sidebarVisibilityMenuOpen ? sidebarVisibilityEntries : contextMenu.state.entries}
          position={contextMenu.state.position}
          showSearch={contextMenu.state.showSearch}
          onClose={() => {
            setSidebarVisibilityMenuOpen(false);
            contextMenu.close();
          }}
        />
      )}

      {/* Drag ghost — looks like a highlighted sidebar row */}
      {folderDragState?.active && (() => {
        const isSmartGhost = folderDragState.draggedNodeId.startsWith('smart:');
        const DefaultIcon = isSmartGhost ? IconBookmark : IconFolder;
        return (
          <div style={{
            position: 'fixed',
            left: folderDragState.ghostX - 20,
            top: folderDragState.ghostY - 13,
            width: 200,
            height: 'var(--sidebar-row-height, 26px)',
            display: 'flex',
            alignItems: 'center',
            gap: 6,
            padding: '0 8px',
            borderRadius: 5,
            background: 'var(--color-surface-active, rgba(248, 249, 251, 0.10))',
            border: '1px solid var(--color-border-primary)',
            color: 'var(--color-text-primary)',
            fontSize: 'var(--font-size-md, 13px)',
            pointerEvents: 'none',
            zIndex: 10000,
            whiteSpace: 'nowrap',
            overflow: 'hidden',
            opacity: 0.9,
            boxShadow: '0 2px 8px rgba(0, 0, 0, 0.2)',
          }}>
            {folderDragState.ghostIcon ? (
              <DynamicIcon name={folderDragState.ghostIcon} size={IC} color={folderDragState.ghostColor ?? undefined} filled />
            ) : (
              <DefaultIcon size={IC} color={folderDragState.ghostColor ?? 'var(--color-text-tertiary)'} stroke={1.2} fill={folderDragState.ghostColor ?? 'currentColor'} fillOpacity={0.15} />
            )}
            <span style={{ overflow: 'hidden', textOverflow: 'ellipsis' }}>{folderDragState.ghostLabel}</span>
          </div>
        );
      })()}
    </div>
  );
}

// ── Helpers ──────────────────────────────────────────────────────

function NodeIcon({ node, expanded }: { node: SidebarNodeDto; expanded: boolean }) {
  if (node.icon) {
    return <DynamicIcon name={node.icon} size={IC} color={node.color} filled />;
  }
  if (isSmartFolderGroup(node)) {
    return <IconLayoutGrid size={IC} color={node.color ?? undefined} stroke={1.2} />;
  }
  const color = node.color ?? undefined;
  const Icon = expanded ? IconFolderOpen : IconFolder;
  return <Icon size={IC} color={color} stroke={1.2} fill={color ?? 'currentColor'} fillOpacity={0.15} />;
}

export function isSmartFolderGroup(node: SidebarNodeDto): boolean {
  return node.kind === 'smart_folder'
    && (node.meta as Record<string, unknown> | null | undefined)?.is_group === true;
}

/** Check if `nodeId` is a descendant of `ancestorId` in the folder tree. */
function isDescendantOf(nodeId: string, ancestorId: string, nodes: SidebarNodeDto[]): boolean {
  const byId = new Map(nodes.map((n) => [n.id, n]));
  let current = byId.get(nodeId);
  const visited = new Set<string>();
  while (current?.parent_id) {
    if (visited.has(current.id)) return false; // cycle guard
    visited.add(current.id);
    if (current.parent_id === ancestorId) return true;
    current = byId.get(current.parent_id);
  }
  return false;
}

/** Filter a set of selected IDs to only top-level parents — remove any child whose ancestor is also selected. */
function deduplicateParentChild(ids: string[], nodes: SidebarNodeDto[]): string[] {
  return ids.filter((id) => !ids.some((other) => other !== id && isDescendantOf(id, other, nodes)));
}

function parseFolderId(nodeId: string): number | null {
  if (!nodeId.startsWith('folder:')) return null;
  const n = parseInt(nodeId.slice(7), 10);
  return isNaN(n) ? null : n;
}

function parseSmartFolderId(nodeId: string): string | null {
  if (!nodeId.startsWith('smart:')) return null;
  return nodeId.slice(6);
}

function parseSmartFolderIdNum(nodeId: string): number | null {
  const s = parseSmartFolderId(nodeId);
  if (s == null) return null;
  const n = parseInt(s, 10);
  return isNaN(n) ? null : n;
}

function parseSmartFolderPredicate(node: SidebarNodeDto): SmartFolderPredicate {
  const predicate = (node.meta as Record<string, unknown> | null | undefined)?.predicate;
  if (
    predicate &&
    typeof predicate === 'object' &&
    Array.isArray((predicate as { groups?: unknown }).groups)
  ) {
    return predicate as SmartFolderPredicate;
  }
  return { groups: [] };
}

function smartFolderInitialFromNode(node: SidebarNodeDto) {
  const meta = node.meta as Record<string, unknown> | null | undefined;
  return {
    id: parseSmartFolderIdNum(node.id) ?? undefined,
    name: node.name,
    parent_id: typeof meta?.parent_id === 'number' || meta?.parent_id === null ? (meta?.parent_id as number | null) : null,
    icon: node.icon ?? null,
    color: node.color ?? null,
    notes: typeof meta?.notes === 'string' ? meta.notes : null,
    predicate: parseSmartFolderPredicate(node),
    display_order: node.sort_order ?? null,
  };
}

function buildSmartFolderPayloadFromNode(
  node: SidebarNodeDto,
  patch: Partial<SmartFolderCommandPayload> = {},
): SmartFolderCommandPayload {
  const initial = smartFolderInitialFromNode(node);
  const pick = <T,>(key: keyof SmartFolderCommandPayload, fallback: T): T => (
    Object.prototype.hasOwnProperty.call(patch, key)
      ? patch[key] as T
      : fallback
  );
  return {
    smart_folder_id: pick('smart_folder_id', initial.id ?? 0),
    name: pick('name', initial.name ?? node.name),
    parent_id: pick('parent_id', initial.parent_id ?? null),
    icon: pick('icon', initial.icon ?? null),
    color: pick('color', initial.color ?? null),
    notes: pick('notes', initial.notes ?? null),
    predicate_json: pick('predicate_json', JSON.stringify(initial.predicate ?? { groups: [] })),
    display_order: pick('display_order', initial.display_order ?? null),
    created_at: pick('created_at', null),
    updated_at: pick('updated_at', null),
  };
}

interface TreeRenderNode {
  node: SidebarNodeDto;
  indent: number;
  hasChildren: boolean;
  /** For each indent level 0..indent-1, true if a vertical line should continue (more siblings below at that depth). */
  treeLines: boolean[];
  /** True if this is the last child of its parent (L-shape, not T-shape). */
  isLastChild: boolean;
}

function buildTreeRenderList(
  nodes: SidebarNodeDto[],
  rootParentId: string,
  collapsed: Set<string>,
): TreeRenderNode[] {
  const childrenMap = new Map<string, SidebarNodeDto[]>();
  for (const node of nodes) {
    const key = node.parent_id ?? rootParentId;
    if (!childrenMap.has(key)) childrenMap.set(key, []);
    childrenMap.get(key)!.push(node);
  }
  for (const children of childrenMap.values()) {
    children.sort((a, b) => (a.sort_order ?? 999) - (b.sort_order ?? 999) || a.name.localeCompare(b.name));
  }
  const result: TreeRenderNode[] = [];
  (function walk(parentId: string, indent: number, ancestorLines: boolean[]) {
    const siblings = childrenMap.get(parentId) ?? [];
    for (let i = 0; i < siblings.length; i++) {
      const node = siblings[i];
      const kids = childrenMap.get(node.id) ?? [];
      const isLast = i === siblings.length - 1;
      // treeLines = ancestor continuation state (immutable copy per node)
      const treeLines = ancestorLines.slice();
      result.push({ node, indent, hasChildren: kids.length > 0, treeLines, isLastChild: isLast });
      if (!collapsed.has(node.id) && kids.length > 0) {
        // Pass down: this node's depth continues if it's not the last sibling
        const childAncestorLines = [...ancestorLines, !isLast];
        walk(node.id, indent + 1, childAncestorLines);
      }
    }
  })(rootParentId, 0, []);
  return result;
}
