/**
 * Sidebar feature root — assembles the sidebar from state atoms.
 *
 * All nodes are driven by the backend sidebar tree. No frontend-invented nodes.
 * Manager surfaces (Tags, Random) are out of scope — see PBI-595, PBI-596.
 */

import { useEffect, useMemo, useCallback, useRef, useState } from 'react';
import { useAtomValue, useSetAtom, getDefaultStore } from 'jotai';
import { folderWatchModalAtom, confirmModalAtom, exportModalAtom, smartFolderModalAtom } from '../../state/modals';
import {
  IconFolder, IconFolderOpen, IconFolderPlus,
  IconCopy, IconUpload, IconDownload,
  IconPhoto, IconInbox, IconTrash,
  IconClock, IconBookmark, IconPin, IconPinnedOff,
} from '@tabler/icons-react';
import type { Icon as TablerIcon } from '@tabler/icons-react';
import {
  IconNewSubfolder, IconRename, IconSort, IconExpand, IconCollapse,
  IconExpandAll, IconChangeIcon, IconWatchFolder,
  IconFolderQuestionCustom, IconBookmarkQuestionCustom,
} from '../../shared/ui/icons/sidebar-menu-icons';
import {
  sidebarNodesAtom, systemNodesAtom, folderNodesAtom,
  smartFolderNodesAtom, sidebarLoadingAtom,
} from '../../state/sidebar';
import { activeNodeIdAtom } from '../../state/navigation';
import { pushHistory } from '../../state/navigationHistory';
import { sidebarController } from '../../controllers/sidebarController';
import {
  bulkFolderDeletionMessage,
  foldersController,
  singleFolderDeletionMessage,
} from '../../controllers/foldersController';
import { smartFoldersController } from '../../controllers/smartFoldersController';
import { pinSidebarItem, unpinSidebarItem, reorderPinnedItems } from '../../platform/sidebarApi';
import { SidebarRow } from '../../shared/ui/SidebarRow';
import { LibrarySwitcherButton } from '../library/LibrarySwitcherButton';
import { ContextMenu, useContextMenu, type MenuEntry } from '../../shared/ui/ContextMenu';
import { ColorPicker } from '../../shared/ui/ColorPicker';
import { IconPicker } from '../../shared/ui/IconPicker';
import { DynamicIcon } from '../../shared/ui/DynamicIcon';
import { useInlineRename } from '../../shared/hooks/useInlineRename';
import { usePersistedSet } from '../../shared/hooks/usePersistedSet';
import type { SidebarNodeDto, SmartFolderCommandPayload, SmartFolderPredicate } from '../../shared/types/canonical';
import styles from './Sidebar.module.css';

const IC = 19;
const FILL = { stroke: 1.2, fill: 'currentColor', fillOpacity: 0.15 } as const;

const SYSTEM_ICONS: Record<string, TablerIcon> = {
  'system:active':        IconPhoto,
  'system:inbox':         IconInbox,
  'system:uncategorized': IconFolderQuestionCustom as unknown as TablerIcon,
  'system:untagged':      IconBookmarkQuestionCustom as unknown as TablerIcon,
  'system:tag_manager':   IconBookmark,
  'system:recent_viewed': IconClock,
  'system:subscriptions': IconDownload,
  'system:duplicates':    IconCopy,
  'system:trash':         IconTrash,
};

const store = getDefaultStore();

/** Fixed display order for all system scopes. */
const SYSTEM_SCOPE_ORDER = [
  'system:active',
  'system:inbox',
  'system:recent_viewed',
  'system:uncategorized',
  'system:untagged',
  'system:tag_manager',
  'system:subscriptions',
  'system:duplicates',
  'system:trash',
];

const LABEL_OVERRIDES: Record<string, string> = {
  'system:active': 'All',
};

export function Sidebar() {
  const nodes = useAtomValue(sidebarNodesAtom);
  const systemNodes = useAtomValue(systemNodesAtom);
  const folderNodes = useAtomValue(folderNodesAtom);
  const smartFolderNodes = useAtomValue(smartFolderNodesAtom);
  const loading = useAtomValue(sidebarLoadingAtom);
  const activeNodeId = useAtomValue(activeNodeIdAtom);
  const setActiveNodeId = useSetAtom(activeNodeIdAtom);
  const setSmartFolderModal = useSetAtom(smartFolderModalAtom);

  const [collapsed, toggleCollapse] = usePersistedSet('picto-sidebar-collapsed');
  const contextMenu = useContextMenu();

  // ── Multi-select state ──
  const [sidebarSelection, setSidebarSelection] = useState<Set<string>>(new Set());
  const lastClickedRef = useRef<string | null>(null);

  // ── Context menu highlight ──
  const [contextMenuNodeId, setContextMenuNodeId] = useState<string | null>(null);
  // Clear highlight when menu closes
  useEffect(() => {
    if (!contextMenu.state && contextMenuNodeId) setContextMenuNodeId(null);
  }, [contextMenu.state, contextMenuNodeId]);

  // Pinned items — flat list from both folders and smart folders (must be before drag useEffect)
  const pinnedNodes = useMemo(() => {
    const allNodes = [...folderNodes, ...smartFolderNodes];
    return allNodes
      .filter((n) => (n.meta as Record<string, unknown> | null)?.pinned === true)
      .sort((a, b) => {
        const ao = ((a.meta as Record<string, unknown> | null)?.pin_order as number) ?? 0;
        const bo = ((b.meta as Record<string, unknown> | null)?.pin_order as number) ?? 0;
        return ao - bo;
      });
  }, [folderNodes, smartFolderNodes]);

  // ── Folder drag reorder ──
  const folderDragRef = useRef<{ nodeId: string; startY: number; fromPinned?: boolean } | null>(null);
  const [folderDragState, setFolderDragState] = useState<{
    active: boolean;
    draggedNodeId: string;
    dropTargetId: string | null;
    dropPosition: 'before' | 'inside' | 'after' | null;
    isPinnedReorder?: boolean;
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
        const el = document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null;
        let dropTargetId: string | null = null;
        let dropPosition: 'before' | 'inside' | 'after' | null = null;
        let isPinnedReorder = false;

        if (drag.fromPinned) {
          // Pinned drag — look for BOTH folder and smart folder targets
          const folderTarget = el?.closest('[data-folder-drop-id]') as HTMLElement | null;
          const smartTarget = el?.closest('[data-smart-drop-id]') as HTMLElement | null;
          let targetRow: HTMLElement | null = null;
          let targetNodeId: string | null = null;
          if (folderTarget) {
            targetRow = folderTarget;
            targetNodeId = `folder:${folderTarget.dataset.folderDropId}`;
          } else if (smartTarget) {
            targetRow = smartTarget;
            targetNodeId = `smart:${smartTarget.dataset.smartDropId}`;
          }
          if (targetRow && targetNodeId && targetNodeId !== drag.nodeId) {
            // Check if target is also pinned → pinned reorder
            const targetIsPinned = pinnedNodes.some((n) => n.id === targetNodeId);
            if (targetIsPinned) {
              isPinnedReorder = true;
              dropTargetId = targetNodeId;
              const rect = targetRow.getBoundingClientRect();
              const ratio = (e.clientY - rect.top) / rect.height;
              dropPosition = ratio < 0.5 ? 'before' : 'after';
            } else {
              // Dropping from pinned onto non-pinned: normal move (only same-type)
              const sameType = (isSmartDrag && targetNodeId.startsWith('smart:')) ||
                (!isSmartDrag && targetNodeId.startsWith('folder:'));
              if (sameType && !isDescendantOf(targetNodeId, drag.nodeId, nodePool)) {
                dropTargetId = targetNodeId;
                const rect = targetRow.getBoundingClientRect();
                const ratio = (e.clientY - rect.top) / rect.height;
                if (ratio < 0.3) dropPosition = 'before';
                else if (ratio > 0.7) dropPosition = 'after';
                else dropPosition = 'inside';
              }
            }
          }
        } else {
          // Normal drag — detect same-type targets only
          const dropAttr = isSmartDrag ? 'data-smart-drop-id' : 'data-folder-drop-id';
          const targetRow = el?.closest(`[${dropAttr}]`) as HTMLElement | null;
          if (targetRow) {
            const rawId = isSmartDrag
              ? targetRow.dataset.smartDropId
              : targetRow.dataset.folderDropId;
            const prefix = isSmartDrag ? 'smart:' : 'folder:';
            const targetNodeId = rawId ? `${prefix}${rawId}` : null;
            // Prevent dropping onto self or any descendant (would create a cycle)
            if (targetNodeId && targetNodeId !== drag.nodeId && !isDescendantOf(targetNodeId, drag.nodeId, nodePool)) {
              dropTargetId = targetNodeId;
              const rect = targetRow.getBoundingClientRect();
              const ratio = (e.clientY - rect.top) / rect.height;
              if (ratio < 0.3) dropPosition = 'before';
              else if (ratio > 0.7) dropPosition = 'after';
              else dropPosition = 'inside';
            }
          }
        }

        setFolderDragState((prev) => prev ? {
          ...prev,
          ghostX: e.clientX,
          ghostY: e.clientY,
          dropTargetId,
          dropPosition,
          isPinnedReorder,
        } : null);
      }
    };

    const onUp = () => {
      const drag = folderDragRef.current;
      folderDragRef.current = null;
      document.body.style.cursor = '';

      if (folderDragState?.active && drag) {
        const { dropTargetId, dropPosition, isPinnedReorder } = folderDragState;
        const isSmartDrag = drag.nodeId.startsWith('smart:');

        if (dropTargetId && dropPosition && isPinnedReorder) {
          // Pinned section reorder — compute new pin_order for all pinned items
          const draggedIds = sidebarSelection.has(drag.nodeId) && sidebarSelection.size > 1
            ? [...sidebarSelection].filter((id) => pinnedNodes.some((n) => n.id === id))
            : [drag.nodeId];
          const movingSet = new Set(draggedIds);
          const movingNodes = pinnedNodes.filter((n) => movingSet.has(n.id));
          const without = pinnedNodes.filter((n) => !movingSet.has(n.id));
          const targetIdx = without.findIndex((n) => n.id === dropTargetId);
          const insertAt = dropPosition === 'after' ? targetIdx + 1 : targetIdx;
          without.splice(insertAt, 0, ...movingNodes);
          const moves: [string, number][] = without.map((n, i) => [n.id, i]);
          void reorderPinnedItems(moves);
        } else if (dropTargetId && dropPosition) {
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
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };
  }, [folderDragState, folderNodes, smartFolderNodes, pinnedNodes, sidebarSelection]);

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
  const pendingRenameRef = useRef<string | null>(null);

  useEffect(() => { sidebarController.ensureLoaded(); }, []);

  // Trigger pending rename when folder nodes update
  useEffect(() => {
    const pendingId = pendingRenameRef.current;
    if (!pendingId) return;
    const node = folderNodes.find((n) => n.id === pendingId);
    if (node) {
      pendingRenameRef.current = null;
      folderRename.startRename(node.id, node.name);
    }
  }, [folderNodes, folderRename]);

  const createFolderAndRename = useCallback(async (parentId?: number | null) => {
    const nodeId = await foldersController.create('New Folder', parentId);
    if (nodeId) {
      pendingRenameRef.current = nodeId;
    }
  }, []);

  const openSmartFolderModal = useCallback((mode: 'create' | 'edit', initial?: {
    id?: number;
    name?: string;
    parent_id?: number | null;
    icon?: string | null;
    color?: string | null;
    notes?: string | null;
    predicate?: SmartFolderPredicate;
    sort_field?: string | null;
    sort_order?: string | null;
    display_order?: number | null;
  }) => {
    setSmartFolderModal({ open: true, mode, initial });
  }, [setSmartFolderModal]);

  const navigate = useCallback((id: string) => {
    setActiveNodeId(id);
    pushHistory(id);
  }, [setActiveNodeId]);

  const toggleCollapseAll = useCallback(() => {
    // If any folder is expanded, collapse all. Otherwise expand all.
    const folderIds = folderNodes.map((n) => n.id);
    const anyExpanded = folderIds.some((id) => !collapsed.has(id));
    if (anyExpanded) {
      folderIds.forEach((id) => { if (!collapsed.has(id)) toggleCollapse(id); });
    } else {
      folderIds.forEach((id) => { if (collapsed.has(id)) toggleCollapse(id); });
    }
  }, [folderNodes, collapsed, toggleCollapse]);

  const orderedSystemNodes = useMemo(() => {
    const byId = new Map(systemNodes.map((n) => [n.id, n]));
    const result: SidebarNodeDto[] = [];
    for (const id of SYSTEM_SCOPE_ORDER) {
      const node = byId.get(id);
      if (node) {
        result.push(node);
      } else if (id === 'system:tag_manager') {
        // Placeholder — not from backend
        result.push({ id, name: 'Tag Manager', count: null, selectable: true, sort_order: 0 } as SidebarNodeDto);
      }
    }
    return result;
  }, [systemNodes]);
  const folderList = useMemo(
    () => buildTreeRenderList(folderNodes, 'section:folders', collapsed),
    [folderNodes, collapsed],
  );
  const smartList = useMemo(
    () => buildTreeRenderList(smartFolderNodes, 'section:smart_folders', collapsed),
    [smartFolderNodes, collapsed],
  );

  /** Multi-select-aware click handler for folder / smart folder rows. */
  const handleRowClick = useCallback((id: string, e: React.MouseEvent) => {
    const isMod = e.metaKey || e.ctrlKey;
    const isShift = e.shiftKey;

    if (isMod) {
      // Toggle in selection
      setSidebarSelection((prev) => {
        const next = new Set(prev);
        if (next.has(id)) next.delete(id); else next.add(id);
        return next;
      });
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
          setSidebarSelection((prev) => {
            const next = new Set(prev);
            for (let i = lo; i <= hi; i++) next.add(ids[i]);
            return next;
          });
          return; // don't navigate
        }
      }
    }

    // Plain click — navigate and clear selection
    setSidebarSelection(new Set());
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
    const entries: MenuEntry[] = [
      { label: 'New Folder', icon: <IconFolderPlus size={14} />, action: () => { void createFolderAndRename(); } },
      { label: 'New Subfolder', icon: <IconNewSubfolder size={14} />, action: () => { void createFolderAndRename(folderId); } },
      { separator: true },
      { label: 'Rename', icon: <IconRename size={14} />, action: () => folderRename.startRename(node.id, node.name) },
      { separator: true },
      { label: 'Import Folder Here...', icon: <IconFolderPlus size={14} />, action: () => {
        void (async () => {
          try {
            const result = await (window as any).picto.dialog.open({
              properties: ['openDirectory'], multiple: false, title: 'Import folder into ' + node.name,
            });
            if (result) {
              const folderPath = typeof result === 'string' ? result : result[0];
              await foldersController.addMedia(folderPath, folderId);
            }
          } catch (err) {
            console.error('[sidebar] import folder failed:', err);
          }
        })();
      } },
      { label: 'Attach Watched Folder...', icon: <IconWatchFolder size={14} />, action: () => {
        store.set(folderWatchModalAtom, { open: true, folderId, initial: {} });
      } },
      ...((node.meta as Record<string, unknown> | null)?.watch_enabled ? [{
        label: 'Remove Watched Folder', icon: <IconWatchFolder size={14} />, danger: true,
        action: () => {
          store.set(confirmModalAtom, {
            open: true, title: 'Remove Watch', danger: true, confirmLabel: 'Remove',
            message: `Stop watching the folder for "${node.name}"?`,
            onConfirm: () => { void foldersController.clearWatchConfig(folderId); },
          });
        },
      } as MenuEntry] : []),
      { separator: true },
      { label: 'Sort by Name', icon: <IconSort size={14} />, action: () => { void foldersController.sortByName(folderId); } },
      { label: isExpanded ? 'Collapse Folder' : 'Expand Folder', icon: isExpanded ? <IconCollapse size={14} /> : <IconExpand size={14} />,
        action: () => { if (hasChildren) toggleCollapse(node.id); },
        disabled: !hasChildren },
      { label: 'Expand/Collapse All', icon: <IconExpandAll size={14} />, action: () => toggleCollapseAll() },
      { separator: true },
      { submenu: true, label: 'Change Icon', icon: <IconChangeIcon size={14} />, children: [
        { custom: true, key: 'folder-icon', render: () => (
          <IconPicker value={node.icon ?? null} onChange={(icon) => { void foldersController.applyIcon(folderId, icon); }} />
        ) },
      ] },
      { custom: true, key: 'folder-color', render: () => (
        <ColorPicker value={node.color ?? null} onChange={(hex) => foldersController.applyColor(folderId, hex)} />
      ) },
      { separator: true },
      { label: 'Duplicate', icon: <IconCopy size={14} />, disabled: true, action: () => {} },
      { label: 'Export...', icon: <IconUpload size={14} />, action: () => {
        store.set(exportModalAtom, {
          open: true, fileCount: node.count ?? 0,
          target: { kind: 'query_results', query: { base_scope: { kind: 'folder', id: folderId } } },
        });
      } },
      { separator: true },
      (node.meta as Record<string, unknown> | null)?.pinned
        ? { label: 'Unpin', icon: <IconPinnedOff size={14} />, action: () => { void unpinSidebarItem(node.id); } }
        : { label: 'Pin to Sidebar', icon: <IconPin size={14} />, action: () => { void pinSidebarItem(node.id); } },
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
  }, [contextMenu, folderRename, collapsed, toggleCollapse]);

  const openSmartFolderMenu = useCallback((e: React.MouseEvent, node: SidebarNodeDto) => {
    setContextMenuNodeId(node.id);
    const sfId = parseSmartFolderId(node.id);
    const sfIdNum = parseSmartFolderIdNum(node.id);
    if (sfId == null) return;
    const currentPayload = buildSmartFolderPayloadFromNode(node);
    const entries: MenuEntry[] = [
      {
        label: 'Edit Smart Folder...',
        icon: <IconFolderOpen size={14} />,
        action: () => openSmartFolderModal('edit', smartFolderInitialFromNode(node)),
      },
      {
        label: 'New Child Smart Folder',
        icon: <IconFolderPlus size={14} />,
        action: () => openSmartFolderModal('create', {
          name: 'New Smart Folder',
          parent_id: sfIdNum,
          predicate: { groups: [] },
        }),
      },
      { separator: true },
      { label: 'Rename', icon: <IconRename size={14} />, action: () => folderRename.startRename(node.id, node.name) },
      { separator: true },
      { submenu: true, label: 'Change Icon', icon: <IconChangeIcon size={14} />, children: [
        { custom: true, key: 'sf-icon', render: () => (
          <IconPicker value={node.icon ?? null} onChange={(icon) => {
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
      { label: 'Duplicate', icon: <IconCopy size={14} />, disabled: true, action: () => {} },
      { label: 'Export...', icon: <IconUpload size={14} />, action: () => {
        if (sfIdNum != null) {
          store.set(exportModalAtom, {
            open: true, fileCount: node.count ?? 0,
            target: { kind: 'query_results', query: { base_scope: { kind: 'smart_folder', id: sfIdNum } } },
          });
        }
      } },
      { separator: true },
      (node.meta as Record<string, unknown> | null)?.pinned
        ? { label: 'Unpin', icon: <IconPinnedOff size={14} />, action: () => { void unpinSidebarItem(node.id); } }
        : { label: 'Pin to Sidebar', icon: <IconPin size={14} />, action: () => { void pinSidebarItem(node.id); } },
      { separator: true },
      { label: 'Delete', icon: <IconTrash size={14} />, danger: true, action: () => {
        store.set(confirmModalAtom, {
          open: true, title: 'Delete Smart Folder', danger: true, confirmLabel: 'Delete',
          message: `Delete "${node.name}"? This only removes the smart folder, not its contents.`,
          onConfirm: () => smartFoldersController.delete(sfId),
        });
      } },
    ];
    contextMenu.open(e, entries);
  }, [contextMenu, folderRename, openSmartFolderModal]);

  // ── Bulk context menu ──────────────────────────────────────────

  const openBulkMenu = useCallback((e: React.MouseEvent) => {
    const sel = sidebarSelection;
    const folderIds = [...sel].filter((id) => id.startsWith('folder:'));
    const smartIds = [...sel].filter((id) => id.startsWith('smart:'));
    const allFolders = smartIds.length === 0;
    const allSmart = folderIds.length === 0;

    const entries: MenuEntry[] = [];

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

    // Pin/Unpin all
    const anyPinned = [...sel].some((id) => {
      const node = [...folderNodes, ...smartFolderNodes].find((n) => n.id === id);
      return (node?.meta as Record<string, unknown> | null)?.pinned === true;
    });
    const anyUnpinned = [...sel].some((id) => {
      const node = [...folderNodes, ...smartFolderNodes].find((n) => n.id === id);
      return (node?.meta as Record<string, unknown> | null)?.pinned !== true;
    });
    if (anyUnpinned) {
      entries.push({ label: 'Pin All', icon: <IconPin size={14} />, action: () => {
        for (const id of sel) void pinSidebarItem(id);
      } });
    }
    if (anyPinned) {
      entries.push({ label: 'Unpin All', icon: <IconPinnedOff size={14} />, action: () => {
        for (const id of sel) void unpinSidebarItem(id);
      } });
    }
    entries.push({ separator: true });

    // Export — first selected folder or smart folder
    if (folderIds.length > 0 || smartIds.length > 0) {
      entries.push({ label: 'Export All...', icon: <IconUpload size={14} />, action: () => {
        // Export the first selected item (bulk export of multiple scopes isn't supported)
        if (folderIds.length > 0) {
          const fid = parseFolderId(folderIds[0]);
          if (fid != null) {
            store.set(exportModalAtom, {
              open: true, fileCount: 0,
              target: { kind: 'query_results', query: { base_scope: { kind: 'folder', id: fid } } },
            });
          }
        } else {
          const sfid = parseSmartFolderIdNum(smartIds[0]);
          if (sfid != null) {
            store.set(exportModalAtom, {
              open: true, fileCount: 0,
              target: { kind: 'query_results', query: { base_scope: { kind: 'smart_folder', id: sfid } } },
            });
          }
        }
      } });
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
  }, [contextMenu, sidebarSelection, smartFolderNodes]);

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
        {orderedSystemNodes.map((node) => {
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
              count={node.count}
              active={activeNodeId === node.id}
              onClick={() => { if (node.selectable) { setSidebarSelection(new Set()); navigate(node.id); } }}
              dropDataAttr={statusDrop ? { key: 'status-drop', value: statusDrop } : undefined}
            />
          );
        })}

        {/* Pinned items — flat list */}
        {pinnedNodes.length > 0 && (
          <>
            <SidebarRow
              variant="section" label="Pinned"
              expanded={!collapsed.has('pinned')}
              onToggle={() => toggleCollapse('pinned')}
            />
            {!collapsed.has('pinned') && pinnedNodes.map((node) => {
              const isFolder = node.kind === 'folder';
              return (
                <SidebarRow
                  key={`pin-${node.id}`}
                  variant={isFolder ? 'folder' : 'smart_folder'}
                  icon={<NodeIcon node={node} expanded={false} />}
                  label={node.name}
                  count={node.count}
                  active={activeNodeId === node.id}
                  selected={sidebarSelection.has(node.id)}
                  contextHighlight={contextMenuNodeId === node.id}
                  dropPosition={folderDragState?.isPinnedReorder && folderDragState?.dropTargetId === node.id ? folderDragState.dropPosition ?? undefined : undefined}
                  onClick={(e) => handleRowClick(node.id, e)}
                  onPointerDown={(e) => {
                    if (e.button !== 0) return;
                    folderDragRef.current = { nodeId: node.id, startY: e.clientY, fromPinned: true };
                  }}
                  onContextMenu={(e) => {
                    if (isFolder) openFolderMenu(e, node);
                    else openSmartFolderMenu(e, node);
                  }}
                  dropDataAttr={isFolder
                    ? { key: 'folder-drop-id', value: String(parseFolderId(node.id) ?? '') }
                    : { key: 'smart-drop-id', value: String(parseSmartFolderIdNum(node.id) ?? '') }
                  }
                />
              );
            })}
          </>
        )}

        {/* Folders */}
        <SidebarRow
          variant="section" label="Folders"
          expanded={!collapsed.has('folders')}
          onToggle={() => toggleCollapse('folders')}
          onAdd={() => { void createFolderAndRename(); }}
          addTooltip="New Folder" addShortcut="Mod+Shift+N"
        />
        {!collapsed.has('folders') && folderList.map(({ node, indent, hasChildren, treeLines, isLastChild }) => (
          <SidebarRow
            key={node.id} variant="folder"
            icon={<NodeIcon node={node} expanded={!collapsed.has(node.id) && hasChildren} />}
            label={folderRename.renamingId === node.id ? undefined : node.name}
            count={folderRename.renamingId === node.id ? undefined : node.count}

            active={activeNodeId === node.id} indent={indent}
            selected={sidebarSelection.has(node.id)}
            hasChildren={hasChildren} expanded={!collapsed.has(node.id)}
            treeLines={treeLines} isLastChild={isLastChild}
            contextHighlight={contextMenuNodeId === node.id}
            dropPosition={!folderDragState?.isPinnedReorder && folderDragState?.dropTargetId === node.id ? folderDragState.dropPosition ?? undefined : undefined}
            onToggleExpand={() => toggleCollapse(node.id)}
            onClick={(e) => handleRowClick(node.id, e)}
            onContextMenu={(e) => handleFolderContextMenu(e, node)}
            onPointerDown={(e) => {
              if (e.button !== 0) return;
              folderDragRef.current = { nodeId: node.id, startY: e.clientY };
            }}
            dropDataAttr={{ key: 'folder-drop-id', value: String(parseFolderId(node.id) ?? '') }}
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
        <SidebarRow
          variant="section" label="Smart Folders"
          expanded={!collapsed.has('smart_folders')}
          onToggle={() => toggleCollapse('smart_folders')}
          onAdd={() => openSmartFolderModal('create', { name: 'New Smart Folder', predicate: { groups: [] } })}
          addTooltip="New Smart Folder"
        />
        {!collapsed.has('smart_folders') && smartList.map(({ node, indent, hasChildren, treeLines, isLastChild }) => (
          <SidebarRow
            key={node.id} variant="smart_folder"
            icon={<NodeIcon node={node} expanded={!collapsed.has(node.id) && hasChildren} />}
            label={folderRename.renamingId === node.id ? undefined : node.name}
            count={folderRename.renamingId === node.id ? undefined : node.count}

            active={activeNodeId === node.id} indent={indent}
            selected={sidebarSelection.has(node.id)}
            hasChildren={hasChildren} expanded={!collapsed.has(node.id)}
            treeLines={treeLines} isLastChild={isLastChild}
            contextHighlight={contextMenuNodeId === node.id}
            dropPosition={!folderDragState?.isPinnedReorder && folderDragState?.dropTargetId === node.id ? folderDragState.dropPosition ?? undefined : undefined}
            onToggleExpand={() => toggleCollapse(node.id)}
            onClick={(e) => handleRowClick(node.id, e)}
            onContextMenu={(e) => handleSmartFolderContextMenu(e, node)}
            onPointerDown={(e) => {
              if (e.button !== 0) return;
              folderDragRef.current = { nodeId: node.id, startY: e.clientY };
            }}
            dropDataAttr={{ key: 'smart-drop-id', value: String(parseSmartFolderIdNum(node.id) ?? '') }}
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

      </div>

      {/* Context menu portal */}
      {contextMenu.state && (
        <ContextMenu
          entries={contextMenu.state.entries}
          position={contextMenu.state.position}
          onClose={contextMenu.close}
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
  const color = node.color ?? undefined;
  const Icon = expanded ? IconFolderOpen : IconFolder;
  return <Icon size={IC} color={color} stroke={1.2} fill={color ?? 'currentColor'} fillOpacity={0.15} />;
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
    sort_field: typeof meta?.sort_field === 'string' ? meta.sort_field : null,
    sort_order: typeof meta?.sort_order === 'string' ? meta.sort_order : null,
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
    sort_field: pick('sort_field', initial.sort_field ?? null),
    sort_order: pick('sort_order', initial.sort_order ?? null),
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
