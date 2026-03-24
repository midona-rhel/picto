/**
 * Sidebar feature root — assembles the sidebar from state atoms.
 *
 * Visual structure matches the legacy sidebar (reference application-style):
 *   1. System scope rows (flat, no section header)
 *   2. "Folders" collapsible section with + button and nested folder tree
 *   3. "Smart Folders" collapsible section with + button and smart folder list
 */

import { useEffect, useState, useCallback, useMemo } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import {
  IconPhoto,
  IconInbox,
  IconFolderQuestion,
  IconTrash,
  IconBookmarkQuestion,
  IconBookmark,
  IconCopy,
  IconArrowsShuffle,
  IconFolder,
  IconFolderOpen,
} from '@tabler/icons-react';
import {
  sidebarNodesAtom,
  scopeCountsAtom,
  folderNodesAtom,
  smartFolderNodesAtom,
  sidebarLoadingAtom,
} from '../../state/sidebar';
import { activeNodeIdAtom } from '../../state/navigation';
import { sidebarController } from '../../controllers/sidebarController';
import { SidebarRow } from '../../shared/ui/SidebarRow';
import { DynamicIcon } from '../../shared/ui/DynamicIcon';
import type { SidebarNodeDto } from '../../shared/types/canonical';
import styles from './Sidebar.module.css';

const ICON_SIZE = 19;

export function Sidebar() {
  const nodes = useAtomValue(sidebarNodesAtom);
  const counts = useAtomValue(scopeCountsAtom);
  const folderNodes = useAtomValue(folderNodesAtom);
  const smartFolderNodes = useAtomValue(smartFolderNodesAtom);
  const loading = useAtomValue(sidebarLoadingAtom);
  const activeNodeId = useAtomValue(activeNodeIdAtom);
  const setActiveNodeId = useSetAtom(activeNodeIdAtom);

  const [collapsedSections, setCollapsedSections] = useState<Set<string>>(new Set());
  const [collapsedFolders, setCollapsedFolders] = useState<Set<string>>(new Set());

  useEffect(() => {
    sidebarController.ensureLoaded();
  }, []);

  const toggleSection = useCallback((key: string) => {
    setCollapsedSections((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }, []);

  const toggleFolder = useCallback((nodeId: string) => {
    setCollapsedFolders((prev) => {
      const next = new Set(prev);
      if (next.has(nodeId)) next.delete(nodeId);
      else next.add(nodeId);
      return next;
    });
  }, []);

  const navigate = useCallback((nodeId: string) => {
    setActiveNodeId(nodeId);
  }, [setActiveNodeId]);

  const folderRenderList = useMemo(
    () => buildTreeRenderList(folderNodes, 'section:folders', collapsedFolders),
    [folderNodes, collapsedFolders],
  );

  const smartFolderRenderList = useMemo(
    () => buildTreeRenderList(smartFolderNodes, 'section:smart_folders', collapsedFolders),
    [smartFolderNodes, collapsedFolders],
  );

  const foldersExpanded = !collapsedSections.has('folders');
  const smartFoldersExpanded = !collapsedSections.has('smart_folders');

  return (
    <div className={styles.root}>
      <div className={styles.scroll}>
        {loading && nodes.length === 0 && (
          <div className={styles.loadingMessage}>Loading…</div>
        )}

        {/* System scopes — flat rows, filled semi-transparent icons */}
        <SidebarRow
          icon={<IconPhoto size={ICON_SIZE} stroke={1.5} fill="currentColor" fillOpacity={0.15} />}
          label="All Active"
          count={counts.active}
          active={activeNodeId === 'system:active'}
          onClick={() => navigate('system:active')}
        />
        <SidebarRow
          icon={<IconInbox size={ICON_SIZE} stroke={1.5} fill="currentColor" fillOpacity={0.15} />}
          label="Inbox"
          count={counts.inbox}
          active={activeNodeId === 'system:inbox'}
          onClick={() => navigate('system:inbox')}
        />
        <SidebarRow
          icon={<IconFolderQuestion size={ICON_SIZE} stroke={1.5} fill="currentColor" fillOpacity={0.15} />}
          label="Uncategorized"
          count={counts.uncategorized}
          active={activeNodeId === 'system:uncategorized'}
          onClick={() => navigate('system:uncategorized')}
        />
        <SidebarRow
          icon={<IconBookmarkQuestion size={ICON_SIZE} stroke={1.5} fill="currentColor" fillOpacity={0.15} />}
          label="Untagged"
          count={counts.untagged}
          active={activeNodeId === 'system:untagged'}
          onClick={() => navigate('system:untagged')}
        />
        <SidebarRow
          icon={<IconBookmark size={ICON_SIZE} stroke={1.5} fill="currentColor" fillOpacity={0.15} />}
          label="Tag Manager"
          active={activeNodeId === 'system:tags'}
          onClick={() => navigate('system:tags')}
        />
        <SidebarRow
          icon={<IconArrowsShuffle size={ICON_SIZE} stroke={1.5} fill="currentColor" fillOpacity={0.15} />}
          label="Random"
          active={activeNodeId === 'system:random'}
          onClick={() => navigate('system:random')}
        />
        <SidebarRow
          icon={<IconCopy size={ICON_SIZE} stroke={1.5} fill="currentColor" fillOpacity={0.15} />}
          label="Duplicates"
          count={counts.duplicates}
          active={activeNodeId === 'system:duplicates'}
          onClick={() => navigate('system:duplicates')}
        />
        <SidebarRow
          icon={<IconTrash size={ICON_SIZE} stroke={1.5} fill="currentColor" fillOpacity={0.15} />}
          label="Trash"
          count={counts.trash}
          active={activeNodeId === 'system:trash'}
          onClick={() => navigate('system:trash')}
        />

        {/* Folders section with + button */}
        <SidebarRow
          variant="section"
          label="Folders"
          expanded={foldersExpanded}
          onToggle={() => toggleSection('folders')}
          onAdd={() => { /* TODO: folder create modal */ }}
        />
        {foldersExpanded && folderRenderList.map(({ node, indent, hasChildren }) => {
          const isExpanded = !collapsedFolders.has(node.id);
          const folderColor = node.color ?? undefined;
          const FolderIcn = node.icon
            ? () => <DynamicIcon name={node.icon!} size={ICON_SIZE} color={node.color} filled />
            : () => (isExpanded && hasChildren)
              ? <IconFolderOpen size={ICON_SIZE} color={folderColor} stroke={1.5} fill={folderColor ?? 'currentColor'} fillOpacity={0.15} />
              : <IconFolder size={ICON_SIZE} color={folderColor} stroke={1.5} fill={folderColor ?? 'currentColor'} fillOpacity={0.15} />;

          return (
            <SidebarRow
              key={node.id}
              variant="folder"
              icon={<FolderIcn />}
              label={node.name}
              count={node.count}
              countStale={node.freshness !== 'exact' && node.freshness !== 'fresh'}
              active={activeNodeId === node.id}
              indent={indent}
              hasChildren={hasChildren}
              expanded={isExpanded}
              onToggleExpand={() => toggleFolder(node.id)}
              onClick={() => navigate(node.id)}
            />
          );
        })}

        {/* Smart Folders section with + button */}
        <SidebarRow
          variant="section"
          label="Smart Folders"
          expanded={smartFoldersExpanded}
          onToggle={() => toggleSection('smart_folders')}
          onAdd={() => { /* TODO: smart folder create modal */ }}
        />
        {smartFoldersExpanded && smartFolderRenderList.map(({ node, indent, hasChildren }) => {
          const isExpanded = !collapsedFolders.has(node.id);
          const iconName = node.icon ?? (isExpanded && hasChildren ? 'IconFolderOpen' : 'IconFolder');

          return (
            <SidebarRow
              key={node.id}
              variant="smart_folder"
              icon={<DynamicIcon name={iconName} size={ICON_SIZE} color={node.color} filled />}
              label={node.name}
              count={node.count}
              countStale={node.freshness !== 'exact' && node.freshness !== 'fresh'}
              active={activeNodeId === node.id}
              indent={indent}
              hasChildren={hasChildren}
              expanded={isExpanded}
              onToggleExpand={() => toggleFolder(node.id)}
              onClick={() => navigate(node.id)}
            />
          );
        })}
      </div>
    </div>
  );
}

// ── Tree rendering helper ────────────────────────────────────────

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
    const parentKey = node.parent_id ?? rootParentId;
    if (!childrenMap.has(parentKey)) childrenMap.set(parentKey, []);
    childrenMap.get(parentKey)!.push(node);
  }

  for (const children of childrenMap.values()) {
    children.sort((a, b) =>
      (a.sort_order ?? 999) - (b.sort_order ?? 999) || a.name.localeCompare(b.name),
    );
  }

  const result: TreeRenderNode[] = [];

  function walk(parentId: string, indent: number) {
    const children = childrenMap.get(parentId) ?? [];
    for (const node of children) {
      const nodeChildren = childrenMap.get(node.id) ?? [];
      result.push({ node, indent, hasChildren: nodeChildren.length > 0 });
      if (!collapsed.has(node.id)) {
        walk(node.id, indent + 1);
      }
    }
  }

  walk(rootParentId, 0);
  return result;
}
