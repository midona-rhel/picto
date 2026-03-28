/**
 * Sidebar feature root — assembles the sidebar from state atoms.
 *
 * All nodes are driven by the backend sidebar tree. No frontend-invented nodes.
 * Manager surfaces (Tags, Random) are out of scope — see PBI-595, PBI-596.
 */

import { useEffect, useMemo, useCallback } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import {
  IconFolder, IconFolderOpen, IconFolderPlus, IconFolderSymlink,
  IconFolderMinus, IconCopy, IconUpload,
  IconPhoto, IconInbox, IconTrash,
  IconClock,
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
import { sidebarController } from '../../controllers/sidebarController';
import { foldersController } from '../../controllers/foldersController';
import { smartFoldersController } from '../../controllers/smartFoldersController';
import { SidebarRow } from '../../shared/ui/SidebarRow';
import { ContextMenu, useContextMenu, type MenuEntry } from '../../shared/ui/ContextMenu';
import { ColorPicker } from '../../shared/ui/ColorPicker';
// TODO: wire IconPicker into "Change Icon..." submenu
// import { IconPicker } from '../../shared/ui/IconPicker';
import { DynamicIcon } from '../../shared/ui/DynamicIcon';
import { useInlineRename } from '../../shared/hooks/useInlineRename';
import { usePersistedSet } from '../../shared/hooks/usePersistedSet';
import type { SidebarNodeDto } from '../../shared/types/canonical';
import styles from './Sidebar.module.css';

const IC = 19;
const FILL = { stroke: 1.2, fill: 'currentColor', fillOpacity: 0.15 } as const;

const SYSTEM_ICONS: Record<string, TablerIcon> = {
  'system:active':        IconPhoto,
  'system:inbox':         IconInbox,
  'system:uncategorized': IconFolderQuestionCustom as unknown as TablerIcon,
  'system:untagged':      IconBookmarkQuestionCustom as unknown as TablerIcon,
  'system:recent_viewed': IconClock,
  'system:duplicates':    IconCopy,
  'system:trash':         IconTrash,
};

const PRIMARY_SCOPES = new Set([
  'system:active', 'system:inbox', 'system:uncategorized',
  'system:untagged', 'system:trash',
]);

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

  const [collapsed, toggleCollapse] = usePersistedSet('picto-sidebar-collapsed');
  const contextMenu = useContextMenu();

  const folderRename = useInlineRename({
    onCommit: (id, name) => {
      const fid = parseFolderId(id);
      if (fid != null) { foldersController.rename(fid, name); return; }
      // TODO: smart folder rename — backend requires full SmartFolder struct for update_smart_folder
    },
  });

  useEffect(() => { sidebarController.ensureLoaded(); }, []);

  const navigate = useCallback((id: string) => setActiveNodeId(id), [setActiveNodeId]);

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

  const primaryNodes = useMemo(
    () => systemNodes.filter((n) => PRIMARY_SCOPES.has(n.id)),
    [systemNodes],
  );
  const secondaryNodes = useMemo(
    () => systemNodes.filter((n) => !PRIMARY_SCOPES.has(n.id)),
    [systemNodes],
  );
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
    const folderId = parseFolderId(node.id);
    if (folderId == null) return;
    const isExpanded = !collapsed.has(node.id);
    const hasChildren = folderList.some(({ node: n }) => n.parent_id === node.id);
    const entries: MenuEntry[] = [
      { label: 'New Folder', icon: <IconFolderPlus size={14} />, action: () => foldersController.create('New Folder') },
      { label: 'New Subfolder', icon: <IconNewSubfolder size={14} />, action: () => foldersController.create('New Folder', folderId) },
      { separator: true },
      { label: 'Rename', icon: <IconRename size={14} />, action: () => folderRename.startRename(node.id, node.name) },
      { label: 'Set Auto-Tags...', icon: <IconAutoTags size={14} />, action: () => { /* TODO: needs auto-tags editor panel */ } },
      { separator: true },
      { label: 'Import Folder Here...', icon: <IconFolderPlus size={14} />, action: () => { /* TODO: needs import dialog */ } },
      { label: 'Attach Watched Folder...', icon: <IconWatchFolder size={14} />, action: () => { /* TODO: needs watch config dialog */ } },
      { separator: true },
      { label: 'Sort', icon: <IconSort size={14} />, action: () => { /* TODO: needs sort submenu */ } },
      { label: isExpanded ? 'Collapse Folder' : 'Expand Folder', icon: isExpanded ? <IconCollapse size={14} /> : <IconExpand size={14} />,
        action: () => { if (hasChildren) toggleCollapse(node.id); },
        disabled: !hasChildren },
      { label: 'Expand/Collapse All', icon: <IconExpandAll size={14} />, action: () => toggleCollapseAll() },
      { separator: true },
      { label: 'Change Icon...', icon: <IconChangeIcon size={14} />, action: () => { /* TODO: needs icon picker submenu */ } },
      { custom: true, key: 'folder-color', render: () => (
        <ColorPicker value={node.color ?? null} onChange={(hex) => foldersController.applyColor(folderId, hex)} />
      ) },
      { separator: true },
      { label: 'Duplicate', icon: <IconCopy size={14} />, action: () => { /* TODO: needs folder duplicate API */ } },
      { label: 'Export...', icon: <IconUpload size={14} />, action: () => { /* TODO: needs export dialog */ } },
      { label: 'Move', icon: <IconFolderSymlink size={14} />, action: () => { /* TODO: needs folder destination picker */ } },
      { separator: true },
      { label: 'Remove Folder', icon: <IconFolderMinus size={14} />, danger: true, action: () => foldersController.delete(folderId) },
    ];
    contextMenu.open(e, entries);
  }, [contextMenu, folderRename, collapsed, toggleCollapse]);

  const openSmartFolderMenu = useCallback((e: React.MouseEvent, node: SidebarNodeDto) => {
    const sfId = parseSmartFolderId(node.id);
    if (sfId == null) return;
    const entries: MenuEntry[] = [
      { label: 'Edit Smart Folder...', icon: <IconFolderOpen size={14} />, action: () => { /* TODO: needs smart folder edit modal */ } },
      { label: 'New Child Smart Folder', icon: <IconFolderPlus size={14} />, action: () => { /* TODO: needs smart folder create modal */ } },
      { separator: true },
      { label: 'Rename', icon: <IconRename size={14} />, action: () => folderRename.startRename(node.id, node.name) },
      { separator: true },
      { label: 'Change Icon...', icon: <IconChangeIcon size={14} />, action: () => { /* TODO: needs icon picker submenu */ } },
      { custom: true, key: 'sf-color', render: () => (
        <ColorPicker value={node.color ?? null} onChange={(_hex) => {
          // TODO: smart folder color — backend requires full SmartFolder struct for update_smart_folder
        }} />
      ) },
      { separator: true },
      { label: 'Duplicate', icon: <IconCopy size={14} />, action: () => { /* TODO: needs smart folder duplicate API */ } },
      { separator: true },
      { label: 'Delete', icon: <IconFolderMinus size={14} />, danger: true, action: () => smartFoldersController.delete(sfId) },
    ];
    contextMenu.open(e, entries);
  }, [contextMenu, folderRename]);

  return (
    <div className={styles.root}>
      <div className={styles.scroll}>
        {loading && nodes.length === 0 && (
          <div className={styles.loadingMessage}>Loading…</div>
        )}

        {/* Primary system scopes */}
        {primaryNodes.map((node) => {
          const ScopeIcon = SYSTEM_ICONS[node.id];
          return (
            <SidebarRow
              key={node.id}
              icon={ScopeIcon ? <ScopeIcon size={IC} {...FILL} /> : undefined}
              label={LABEL_OVERRIDES[node.id] ?? node.name}
              count={node.count}
              active={activeNodeId === node.id}
              onClick={() => node.selectable && navigate(node.id)}
            />
          );
        })}

        {/* Secondary system scopes */}
        {secondaryNodes.length > 0 && (
          <>
            <div className={styles.separator} />
            {secondaryNodes.map((node) => {
              const ScopeIcon = SYSTEM_ICONS[node.id];
              return (
                <SidebarRow
                  key={node.id}
                  icon={ScopeIcon ? <ScopeIcon size={IC} {...FILL} /> : undefined}
                  label={LABEL_OVERRIDES[node.id] ?? node.name}
                  active={activeNodeId === node.id}
                  onClick={() => node.selectable && navigate(node.id)}
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
          onAdd={() => foldersController.create('New Folder')}
        />
        {!collapsed.has('folders') && folderList.map(({ node, indent, hasChildren }) => (
          <SidebarRow
            key={node.id} variant="folder"
            icon={<NodeIcon node={node} expanded={!collapsed.has(node.id) && hasChildren} />}
            label={folderRename.renamingId === node.id ? undefined : node.name}
            count={folderRename.renamingId === node.id ? undefined : node.count}
            countStale={node.freshness !== 'exact' && node.freshness !== 'fresh'}
            active={activeNodeId === node.id} indent={indent}
            hasChildren={hasChildren} expanded={!collapsed.has(node.id)}
            onToggleExpand={() => toggleCollapse(node.id)}
            onClick={() => navigate(node.id)}
            onContextMenu={(e) => openFolderMenu(e, node)}
          >
            {folderRename.renamingId === node.id ? (
              <input
                ref={folderRename.inputRef}
                className={styles.renameInput}
                value={folderRename.renameValue}
                onChange={(e) => folderRename.setRenameValue(e.target.value)}
                onKeyDown={folderRename.handleKeyDown}
                onBlur={folderRename.commitRename}
              />
            ) : undefined}
          </SidebarRow>
        ))}

        {/* Smart Folders */}
        <SidebarRow
          variant="section" label="Smart Folders"
          expanded={!collapsed.has('smart_folders')}
          onToggle={() => toggleCollapse('smart_folders')}
          onAdd={() => { /* TODO: smart folder create modal */ }}
        />
        {!collapsed.has('smart_folders') && smartList.map(({ node, indent, hasChildren }) => (
          <SidebarRow
            key={node.id} variant="smart_folder"
            icon={<NodeIcon node={node} expanded={!collapsed.has(node.id) && hasChildren} />}
            label={node.name} count={node.count}
            countStale={node.freshness !== 'exact' && node.freshness !== 'fresh'}
            active={activeNodeId === node.id} indent={indent}
            hasChildren={hasChildren} expanded={!collapsed.has(node.id)}
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

function parseFolderId(nodeId: string): number | null {
  if (!nodeId.startsWith('folder:')) return null;
  const n = parseInt(nodeId.slice(7), 10);
  return isNaN(n) ? null : n;
}

function parseSmartFolderId(nodeId: string): string | null {
  if (!nodeId.startsWith('smart:')) return null;
  return nodeId.slice(6);
}

// TODO: restore parseSmartFolderIdNum when smart folder partial update is available
// function parseSmartFolderIdNum(nodeId: string): number | null {
//   const s = parseSmartFolderId(nodeId);
//   if (s == null) return null;
//   const n = parseInt(s, 10);
//   return isNaN(n) ? null : n;
// }

interface TreeRenderNode {
  node: SidebarNodeDto;
  indent: number;
  hasChildren: boolean;
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
  (function walk(parentId: string, indent: number) {
    for (const node of childrenMap.get(parentId) ?? []) {
      const kids = childrenMap.get(node.id) ?? [];
      result.push({ node, indent, hasChildren: kids.length > 0 });
      if (!collapsed.has(node.id)) walk(node.id, indent + 1);
    }
  })(rootParentId, 0);
  return result;
}
