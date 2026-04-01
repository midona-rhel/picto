/**
 * FolderPickerPanel — floating glass panel for selecting folders.
 *
 * Tree view from sidebar nodes. Search filters with ancestor auto-expansion.
 * All folders auto-expanded on open.
 */

import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import { IconSearch, IconCheck, IconChevronRight, IconFolder, IconLayoutSidebar } from '@tabler/icons-react';
import { OverlayShell } from '../../shared/ui/OverlayShell';
import { folderPickerPortalAtom } from '../../state/portals';
import { selectionTargetAtom } from '../../state/selection';
import { folderNodesAtom } from '../../state/sidebar';
import * as entityMutations from '../../controllers/entityMutations';
import type { SidebarNodeDto } from '../../shared/types/canonical';
import shellStyles from '../../shared/ui/OverlayShell/OverlayShell.module.css';
import styles from './FolderPickerPanel.module.css';

// ── Tree node builder ──

interface TreeNode {
  node: SidebarNodeDto;
  folderId: number;
  children: TreeNode[];
  depth: number;
}

const FOLDER_ROOT_PARENT = 'section:folders';

function buildTree(nodes: SidebarNodeDto[]): TreeNode[] {
  const childrenOf = new Map<string, SidebarNodeDto[]>();
  for (const n of nodes) {
    const pid = n.parent_id ?? FOLDER_ROOT_PARENT;
    if (!childrenOf.has(pid)) childrenOf.set(pid, []);
    childrenOf.get(pid)!.push(n);
  }
  for (const children of childrenOf.values()) {
    children.sort((a, b) => (a.sort_order ?? 999) - (b.sort_order ?? 999) || a.name.localeCompare(b.name));
  }

  function build(parentId: string, depth: number): TreeNode[] {
    const children = childrenOf.get(parentId) ?? [];
    return children.map((n) => ({
      node: n,
      folderId: parseInt(n.id.slice(7), 10),
      children: build(n.id, depth + 1),
      depth,
    }));
  }

  return build(FOLDER_ROOT_PARENT, 0);
}

function flattenTree(roots: TreeNode[]): TreeNode[] {
  const result: TreeNode[] = [];
  function walk(nodes: TreeNode[]) {
    for (const n of nodes) { result.push(n); walk(n.children); }
  }
  walk(roots);
  return result;
}

function allIds(roots: TreeNode[]): Set<string> {
  const ids = new Set<string>();
  function walk(nodes: TreeNode[]) {
    for (const n of nodes) { ids.add(n.node.id); walk(n.children); }
  }
  walk(roots);
  return ids;
}

function matchesSearch(node: TreeNode, q: string): boolean {
  if (node.node.name.toLowerCase().includes(q)) return true;
  return node.children.some((c) => matchesSearch(c, q));
}

function expandedForSearch(roots: TreeNode[], q: string): Set<string> {
  const ids = new Set<string>();
  function walk(nodes: TreeNode[]): boolean {
    let any = false;
    for (const n of nodes) {
      const childMatch = walk(n.children);
      const selfMatch = n.node.name.toLowerCase().includes(q);
      if (selfMatch || childMatch) { ids.add(n.node.id); any = true; }
    }
    return any;
  }
  walk(roots);
  return ids;
}

// ── Component ──

export function FolderPickerPanel() {
  const portalState = useAtomValue(folderPickerPortalAtom);
  const setPortalState = useSetAtom(folderPickerPortalAtom);
  const target = useAtomValue(selectionTargetAtom);
  const folderNodes = useAtomValue(folderNodesAtom);
  const open = portalState.open;
  const anchorPosition = portalState.anchor ?? null;
  const closePortal = useCallback(() => setPortalState({ open: false }), [setPortalState]);

  const [pinned, setPinned] = useState(false);
  const [query, setQuery] = useState('');
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const searchRef = useRef<HTMLInputElement>(null);

  // Build tree from flat folder nodes
  const tree = useMemo(() => buildTree(folderNodes), [folderNodes]);

  // Auto-expand all on open
  useEffect(() => {
    if (open) {
      setQuery('');
      setSelected(new Set());
      setExpanded(allIds(tree));
      setTimeout(() => searchRef.current?.focus(), 50);
    }
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps

  // Search: auto-expand matching ancestors
  const searchLower = query.trim().toLowerCase();
  const searchExpanded = useMemo(() => {
    if (!searchLower) return null;
    return expandedForSearch(tree, searchLower);
  }, [tree, searchLower]);

  const effectiveExpanded = searchExpanded ?? expanded;

  const toggleExpand = useCallback((nodeId: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(nodeId)) next.delete(nodeId);
      else next.add(nodeId);
      return next;
    });
  }, []);

  const toggleFolder = useCallback((folderId: number) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(folderId)) next.delete(folderId);
      else next.add(folderId);
      return next;
    });
  }, []);

  const applyFolders = useCallback(() => {
    if (!target || selected.size === 0) return;
    for (const folderId of selected) {
      void entityMutations.updateTargetFolderMembership(target, folderId, 'add');
    }
    if (!pinned) closePortal();
    setSelected(new Set());
  }, [target, selected, pinned, closePortal]);

  if (!open) return null;

  // Render tree recursively
  function renderNode(treeNode: TreeNode): React.ReactNode[] {
    const { node, folderId, children, depth } = treeNode;
    const hasChildren = children.length > 0;
    const isExpanded = effectiveExpanded.has(node.id);
    const isSelected = selected.has(folderId);

    // In search mode, hide nodes that don't match and have no matching descendants
    if (searchLower && !matchesSearch(treeNode, searchLower)) return [];

    const result: React.ReactNode[] = [
      <div
        key={node.id}
        className={styles.folderRow}
        style={{ paddingLeft: 6 + depth * 20 }}
        onClick={() => toggleFolder(folderId)}
      >
        {hasChildren ? (
          <div
            className={`${styles.expandBtn} ${isExpanded ? styles.expandBtnExpanded : ''}`}
            onClick={(e) => { e.stopPropagation(); toggleExpand(node.id); }}
          >
            <IconChevronRight size={12} />
          </div>
        ) : (
          <div className={styles.expandBtnPlaceholder} />
        )}
        <div className={`${shellStyles.checkBox} ${isSelected ? shellStyles.checkBoxChecked : ''}`}>
          {isSelected && <IconCheck size={10} />}
        </div>
        <IconFolder size={14} className={styles.folderIcon} style={node.color ? { color: node.color } : undefined} />
        <span className={styles.folderName}>
          {searchLower ? highlightMatch(node.name, searchLower) : node.name}
        </span>
        {node.count != null && <span className={styles.folderCount}>{node.count.toLocaleString()}</span>}
      </div>,
    ];

    if (isExpanded) {
      for (const child of children) result.push(...renderNode(child));
    }

    return result;
  }

  return (
    <OverlayShell
      open={open}
      onClose={closePortal}
      width={360}
      pinned={pinned}
      anchorPosition={anchorPosition}
      onPinnedChange={setPinned}
      header={
        <div className={shellStyles.searchRow} style={{ flex: 1 }}>
          <IconSearch size={14} className={shellStyles.searchIcon} />
          <input
            ref={searchRef}
            className={shellStyles.searchInput}
            placeholder="Search folders..."
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
      }
      footer={
        <>
          <span className={shellStyles.kbdHint}>
            <span className={shellStyles.kbd}>Esc</span> close
          </span>
          <div className={shellStyles.footerBtnGroup}>
            {selected.size > 0 && (
              <button
                className={`${shellStyles.footerBtn} ${shellStyles.footerBtnPrimary}`}
                onClick={applyFolders}
                type="button"
              >
                Add ({selected.size})
              </button>
            )}
          </div>
        </>
      }
    >
      <div className={styles.folderList}>
        {tree.length === 0 ? (
          <div className={styles.emptyState}>No folders</div>
        ) : (
          tree.flatMap((root) => renderNode(root))
        )}
      </div>
    </OverlayShell>
  );
}

function highlightMatch(text: string, q: string): React.ReactNode {
  const idx = text.toLowerCase().indexOf(q);
  if (idx < 0) return text;
  return (
    <>
      {text.slice(0, idx)}
      <span className={styles.matchHighlight}>{text.slice(idx, idx + q.length)}</span>
      {text.slice(idx + q.length)}
    </>
  );
}
