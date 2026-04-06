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
  IconFolder, IconFolderOpen, IconFolderPlus, IconFolderSymlink,
  IconCopy, IconUpload, IconDownload,
  IconPhoto, IconInbox, IconTrash,
  IconClock, IconBookmark,
} from '@tabler/icons-react';
import type { Icon as TablerIcon } from '@tabler/icons-react';
import {
  IconNewSubfolder, IconRename, IconSort, IconExpand, IconCollapse,
  IconExpandAll, IconChangeIcon, IconAutoTags, IconWatchFolder,
  IconFolderQuestionCustom, IconBookmarkQuestionCustom,
} from '../../shared/ui/icons/sidebar-menu-icons';
import {
  sidebarNodesAtom, systemNodesAtom, folderNodesAtom,
  smartFolderNodesAtom, sidebarLoadingAtom,
} from '../../state/sidebar';
import { activeNodeIdAtom } from '../../state/navigation';
import { pushHistory } from '../../state/navigationHistory';
import * as api from '../../platform/api';
import { sidebarController } from '../../controllers/sidebarController';
import { foldersController } from '../../controllers/foldersController';
import { smartFoldersController } from '../../controllers/smartFoldersController';
import { SidebarRow } from '../../shared/ui/SidebarRow';
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
  } | null>(null);

  // Global pointer listeners for folder drag
  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      const drag = folderDragRef.current;
      if (!drag) return;
      const dy = e.clientY - drag.startY;
      if (!folderDragState?.active && Math.abs(dy) < 5) return;

      // Activate drag
      const node = folderNodes.find((n) => n.id === drag.nodeId);
      if (!folderDragState?.active) {
        setFolderDragState({
          active: true,
          draggedNodeId: drag.nodeId,
          dropTargetId: null,
          dropPosition: null,
          ghostX: e.clientX,
          ghostY: e.clientY,
          ghostLabel: node?.name ?? 'Folder',
        });
        document.body.style.cursor = 'grabbing';
      } else {
        // Find drop target via elementFromPoint
        const el = document.elementFromPoint(e.clientX, e.clientY) as HTMLElement | null;
        const targetRow = el?.closest('[data-folder-drop-id]') as HTMLElement | null;
        let dropTargetId: string | null = null;
        let dropPosition: 'before' | 'inside' | 'after' | null = null;

        if (targetRow) {
          const fid = targetRow.dataset.folderDropId;
          const targetNodeId = fid ? `folder:${fid}` : null;
          // Prevent dropping onto self or any descendant (would create a cycle)
          if (targetNodeId && targetNodeId !== drag.nodeId && !isDescendantOf(targetNodeId, drag.nodeId, folderNodes)) {
            dropTargetId = targetNodeId;
            const rect = targetRow.getBoundingClientRect();
            const ratio = (e.clientY - rect.top) / rect.height;
            if (ratio < 0.25) dropPosition = 'before';
            else if (ratio > 0.75) dropPosition = 'after';
            else dropPosition = 'inside';
          }
        }

        setFolderDragState((prev) => prev ? {
          ...prev,
          ghostX: e.clientX,
          ghostY: e.clientY,
          dropTargetId,
          dropPosition,
        } : null);
      }
    };

    const onUp = () => {
      const drag = folderDragRef.current;
      folderDragRef.current = null;
      document.body.style.cursor = '';

      if (folderDragState?.active && drag) {
        const { dropTargetId, dropPosition } = folderDragState;
        if (dropTargetId && dropPosition) {
          const draggedFolderId = parseFolderId(drag.nodeId);
          const targetFolderId = parseFolderId(dropTargetId);
          if (draggedFolderId != null && targetFolderId != null) {
            const targetNode = folderNodes.find((n) => n.id === dropTargetId);
            if (dropPosition === 'inside') {
              // Reparent into target
              void api.moveFolder(draggedFolderId, targetFolderId, []);
            } else {
              // Reorder: same parent, build sibling order
              const targetParentId = targetNode?.parent_id ?? null;
              const parentFolderId = targetParentId ? parseFolderId(targetParentId) : null;
              const siblings = folderNodes
                .filter((n) => n.parent_id === targetParentId)
                .sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0));
              // Remove dragged, insert at target position
              const without = siblings.filter((s) => s.id !== drag.nodeId);
              const targetIdx = without.findIndex((s) => s.id === dropTargetId);
              const insertAt = dropPosition === 'after' ? targetIdx + 1 : targetIdx;
              const draggedNode = siblings.find((s) => s.id === drag.nodeId);
              if (draggedNode) {
                without.splice(insertAt, 0, draggedNode);
              }
              const moves: [number, number][] = without.map((s, i) => [parseFolderId(s.id)!, i]);
              void api.moveFolder(draggedFolderId, parentFolderId, moves);
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
  }, [folderDragState, folderNodes]);

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
      { label: 'Set Auto-Tags...', icon: <IconAutoTags size={14} />, action: () => { /* TODO: needs auto-tags editor panel */ } },
      { separator: true },
      { label: 'Import Folder Here...', icon: <IconFolderPlus size={14} />, action: () => {
        void (async () => {
          try {
            console.log('[sidebar] opening directory picker for import into folder', folderId);
            const result = await (window as any).picto.dialog.open({
              properties: ['openDirectory'], multiple: false, title: 'Import folder into ' + node.name,
            });
            console.log('[sidebar] dialog result:', result);
            if (result) {
              const folderPath = typeof result === 'string' ? result : result[0];
              console.log('[sidebar] importing folder:', folderPath, '→ parent:', folderId);
              await api.importFolder(folderPath, { parent_folder_id: folderId, preserve_structure: true });
              console.log('[sidebar] import_folder dispatched');
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
            onConfirm: () => { void api.clearFolderWatchConfig(folderId); },
          });
        },
      } as MenuEntry] : []),
      { separator: true },
      { label: 'Sort by Name', icon: <IconSort size={14} />, action: () => { void api.reorderFolderItems(folderId, { sort_by: 'name', direction: 'asc' }); } },
      { label: isExpanded ? 'Collapse Folder' : 'Expand Folder', icon: isExpanded ? <IconCollapse size={14} /> : <IconExpand size={14} />,
        action: () => { if (hasChildren) toggleCollapse(node.id); },
        disabled: !hasChildren },
      { label: 'Expand/Collapse All', icon: <IconExpandAll size={14} />, action: () => toggleCollapseAll() },
      { separator: true },
      { submenu: true, label: 'Change Icon', icon: <IconChangeIcon size={14} />, children: [
        { custom: true, key: 'folder-icon', render: () => (
          <IconPicker value={node.icon ?? null} onChange={(icon) => { void api.updateFolder(folderId, { icon }); }} />
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
      { label: 'Move', icon: <IconFolderSymlink size={14} />, action: () => { /* TODO: needs folder destination picker */ } },
      { separator: true },
      { label: 'Delete', icon: <IconTrash size={14} />, danger: true, action: () => {
        store.set(confirmModalAtom, {
          open: true, title: 'Delete Folder', danger: true, confirmLabel: 'Delete',
          message: `Delete "${node.name}"? Files inside will not be deleted.`,
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

  return (
    <div className={styles.root}>
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
              onClick={() => node.selectable && navigate(node.id)}
              dropDataAttr={statusDrop ? { key: 'status-drop', value: statusDrop } : undefined}
            />
          );
        })}

        {/* Folders */}
        <SidebarRow
          variant="section" label="Folders"
          expanded={!collapsed.has('folders')}
          onToggle={() => toggleCollapse('folders')}
          onAdd={() => { void createFolderAndRename(); }}
        />
        {!collapsed.has('folders') && folderList.map(({ node, indent, hasChildren, treeLines, isLastChild }) => (
          <SidebarRow
            key={node.id} variant="folder"
            icon={<NodeIcon node={node} expanded={!collapsed.has(node.id) && hasChildren} />}
            label={folderRename.renamingId === node.id ? undefined : node.name}
            count={folderRename.renamingId === node.id ? undefined : node.count}

            active={activeNodeId === node.id} indent={indent}
            hasChildren={hasChildren} expanded={!collapsed.has(node.id)}
            treeLines={treeLines} isLastChild={isLastChild}
            contextHighlight={contextMenuNodeId === node.id}
            dropPosition={folderDragState?.dropTargetId === node.id ? folderDragState.dropPosition ?? undefined : undefined}
            onToggleExpand={() => toggleCollapse(node.id)}
            onClick={() => navigate(node.id)}
            onContextMenu={(e) => openFolderMenu(e, node)}
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
        />
        {!collapsed.has('smart_folders') && smartList.map(({ node, indent, hasChildren, treeLines, isLastChild }) => (
          <SidebarRow
            key={node.id} variant="smart_folder"
            icon={<NodeIcon node={node} expanded={!collapsed.has(node.id) && hasChildren} />}
            label={node.name} count={node.count}

            active={activeNodeId === node.id} indent={indent}
            hasChildren={hasChildren} expanded={!collapsed.has(node.id)}
            treeLines={treeLines} isLastChild={isLastChild}
            contextHighlight={contextMenuNodeId === node.id}
            onToggleExpand={() => toggleCollapse(node.id)}
            onClick={() => navigate(node.id)}
            onContextMenu={(e) => openSmartFolderMenu(e, node)}
          />
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

      {/* Folder drag ghost */}
      {folderDragState?.active && (
        <div style={{
          position: 'fixed',
          left: folderDragState.ghostX + 14,
          top: folderDragState.ghostY + 10,
          padding: '3px 8px',
          borderRadius: 4,
          background: 'var(--glass-bg, rgba(24, 25, 28, 0.92))',
          backdropFilter: 'blur(12px)',
          border: '1px solid var(--color-border-secondary)',
          color: 'var(--color-text-primary)',
          fontSize: 12,
          fontWeight: 500,
          pointerEvents: 'none',
          zIndex: 10000,
          whiteSpace: 'nowrap',
        }}>
          {folderDragState.ghostLabel}
        </div>
      )}
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
