/**
 * FolderTree — reusable folder hierarchy with optional checkboxes.
 *
 * Builds a tree from flat SidebarNodeDto[], handles expand/collapse,
 * search filtering with ancestor auto-expansion, and checkbox selection.
 */

import { useState, useEffect, useMemo, useCallback, type ReactNode } from 'react';
import { IconChevronRight, IconFolder, IconCheck } from '@tabler/icons-react';
import type { SidebarNodeDto } from '../../types/canonical';
import checkStyles from '../OverlayShell/OverlayShell.module.css';
import styles from './FolderTree.module.css';

// ── Tree types ──

export interface TreeNode {
  node: SidebarNodeDto;
  folderId: number;
  children: TreeNode[];
  depth: number;
}

const FOLDER_ROOT_PARENT = 'section:folders';

export function buildTree(nodes: SidebarNodeDto[]): TreeNode[] {
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
    return (childrenOf.get(parentId) ?? []).map((n) => ({
      node: n,
      folderId: parseInt(n.id.slice(7), 10),
      children: build(n.id, depth + 1),
      depth,
    }));
  }
  return build(FOLDER_ROOT_PARENT, 0);
}

function allIds(roots: TreeNode[]): Set<string> {
  const ids = new Set<string>();
  (function walk(nodes: TreeNode[]) {
    for (const n of nodes) { ids.add(n.node.id); walk(n.children); }
  })(roots);
  return ids;
}

function matchesSearch(node: TreeNode, q: string): boolean {
  if (node.node.name.toLowerCase().includes(q)) return true;
  return node.children.some((c) => matchesSearch(c, q));
}

function expandedForSearch(roots: TreeNode[], q: string): Set<string> {
  const ids = new Set<string>();
  (function walk(nodes: TreeNode[]): boolean {
    let any = false;
    for (const n of nodes) {
      const childMatch = walk(n.children);
      const selfMatch = n.node.name.toLowerCase().includes(q);
      if (selfMatch || childMatch) { ids.add(n.node.id); any = true; }
    }
    return any;
  })(roots);
  return ids;
}

function highlightMatch(text: string, q: string): ReactNode {
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

// ── Component ──

export interface FolderTreeProps {
  nodes: SidebarNodeDto[];
  selected: Set<number>;
  onToggle: (folderId: number, event: React.MouseEvent) => void;
  search?: string;
  /** Show checkboxes. Default true. */
  checkable?: boolean;
  /** Folder IDs the entity is already a member of — shown as non-toggleable context. */
  memberOf?: Set<number>;
}

/** Flatten visible tree into ordered folder IDs (respects expand/collapse + search). */
export function flattenVisibleIds(
  tree: TreeNode[],
  expanded: Set<string>,
  searchLower: string,
): number[] {
  const ids: number[] = [];
  function walk(nodes: TreeNode[]) {
    for (const n of nodes) {
      if (searchLower && !matchesSearch(n, searchLower)) continue;
      ids.push(n.folderId);
      if (expanded.has(n.node.id)) walk(n.children);
    }
  }
  walk(tree);
  return ids;
}

export function FolderTree({ nodes, selected, onToggle, search = '', checkable = true, memberOf }: FolderTreeProps) {
  const tree = useMemo(() => buildTree(nodes), [nodes]);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  // Auto-expand all on mount / when tree changes
  useEffect(() => { setExpanded(allIds(tree)); }, [tree]);

  const searchLower = search.trim().toLowerCase();
  const searchExp = useMemo(() => searchLower ? expandedForSearch(tree, searchLower) : null, [tree, searchLower]);
  const effectiveExpanded = searchExp ?? expanded;

  const toggleExpand = useCallback((nodeId: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(nodeId)) next.delete(nodeId);
      else next.add(nodeId);
      return next;
    });
  }, []);

  function renderNode(treeNode: TreeNode): ReactNode[] {
    const { node, folderId, children, depth } = treeNode;
    const hasChildren = children.length > 0;
    const isExpanded = effectiveExpanded.has(node.id);
    const isSelected = selected.has(folderId);
    const isMember = memberOf?.has(folderId) ?? false;

    if (searchLower && !matchesSearch(treeNode, searchLower)) return [];

    const result: ReactNode[] = [
      <div
        key={node.id}
        className={`${styles.row} ${isMember ? styles.rowMember : ''}`}
        style={{ paddingLeft: 6 + depth * 20 }}
        onClick={isMember ? undefined : (e) => onToggle(folderId, e)}
      >
        {hasChildren ? (
          <div
            className={`${styles.expandBtn} ${isExpanded ? styles.expandBtnExpanded : ''}`}
            onClick={(e) => { e.stopPropagation(); toggleExpand(node.id); }}
          >
            <IconChevronRight size={12} />
          </div>
        ) : (
          <div className={styles.expandPlaceholder} />
        )}
        {checkable && (
          isMember ? (
            <div className={`${checkStyles.checkBox} ${styles.checkMember}`}>
              <IconCheck size={10} />
            </div>
          ) : (
            <div className={`${checkStyles.checkBox} ${isSelected ? checkStyles.checkBoxChecked : ''}`}>
              {isSelected && <IconCheck size={10} />}
            </div>
          )
        )}
        <IconFolder size={14} className={styles.folderIcon} style={node.color ? { color: node.color } : undefined} />
        <span className={styles.name}>
          {searchLower ? highlightMatch(node.name, searchLower) : node.name}
        </span>
        {isMember && <span className={styles.memberBadge}>Member</span>}
        {node.count != null && <span className={styles.count}>{node.count.toLocaleString()}</span>}
      </div>,
    ];

    if (isExpanded) {
      for (const child of children) result.push(...renderNode(child));
    }

    return result;
  }

  if (tree.length === 0) return <div className={styles.empty}>No folders</div>;

  return (
    <div className={styles.tree}>
      {tree.flatMap((root) => renderNode(root))}
    </div>
  );
}
