/**
 * FolderTree — reusable folder hierarchy with optional checkboxes.
 *
 * Builds a tree from flat SidebarNodeDto[], handles expand/collapse,
 * search filtering with ancestor auto-expansion, and checkbox selection.
 */

import { useState, useEffect, useMemo, useCallback, type ReactNode } from 'react';
import { IconChevronRight, IconFolder, IconCheck, IconX } from '@tabler/icons-react';
import type { SidebarNodeDto } from '../../types/canonical';
import checkStyles from '../OverlayShell/OverlayShell.module.css';
import styles from './FolderTree.module.css';
import { t } from '../../../i18n';

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
  excluded?: Set<number>;
  /** Use filter-specific include/exclude selection semantics. */
  filterSelection?: boolean;
  onExclude?: (folderId: number, event: React.MouseEvent) => void;
  onContextMenu?: (folderId: number, event: React.MouseEvent) => void;
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

export function FolderTree({ nodes, selected, onToggle, search = '', checkable = true, memberOf, excluded, filterSelection = false, onExclude, onContextMenu }: FolderTreeProps) {
  const tree = useMemo(() => buildTree(nodes), [nodes]);
  const namesById = useMemo(() => new Map(nodes.map((node) => [node.id, node.name])), [nodes]);
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

  function renderNode(
    treeNode: TreeNode,
    ancestorContinues: boolean[] = [],
    isLastChild = true,
  ): ReactNode[] {
    const { node, folderId, children, depth } = treeNode;
    const hasChildren = children.length > 0;
    const isExpanded = effectiveExpanded.has(node.id);
    const isSelected = selected.has(folderId);
    const isMember = memberOf?.has(folderId) ?? false;
    const isExcluded = excluded?.has(folderId) ?? false;
    const parentName = node.parent_id?.startsWith('folder:') ? namesById.get(node.parent_id) : undefined;

    if (searchLower && !matchesSearch(treeNode, searchLower)) return [];

    const result: ReactNode[] = [
      <div
        key={node.id}
        className={`${styles.row} ${isSelected ? styles.rowSelected : ''} ${isMember ? styles.rowMember : ''} ${isExcluded ? styles.rowExcluded : ''}`}
        style={{
          paddingLeft: 4 + depth * 24,
          '--folder-row-indent': `${4 + depth * 24}px`,
        } as React.CSSProperties}
        onClick={isMember ? undefined : (e) => onToggle(folderId, e)}
        onContextMenu={onExclude || onContextMenu ? (event) => {
          event.preventDefault();
          event.stopPropagation();
          if (onExclude) onExclude(folderId, event);
          else onContextMenu?.(folderId, event);
        } : undefined}
      >
        {depth > 1 && ancestorContinues.map((continues, index) => continues ? (
          <span
            key={index}
            className={styles.treeGuide}
            style={{ left: 16 + index * 24 }}
            data-folder-tree-guide
          />
        ) : null)}
        {depth > 0 && (
          <svg
            className={styles.treeBranch}
            style={{ left: 16 + (depth - 1) * 24 }}
            viewBox="0 0 16 26"
            preserveAspectRatio="none"
            fill="none"
            aria-hidden="true"
            data-folder-tree-branch={isLastChild ? 'last' : 'middle'}
          >
            {isLastChild ? (
              <path d="M0 0 V9.5 A3.5 3.5 0 0 0 3.5 13 H16" />
            ) : (
              <path d="M0 0 V26 M0 13 H16" />
            )}
          </svg>
        )}
        {checkable && (
          <div className={styles.checkSlot}>
            {isMember ? (
              <div className={`${checkStyles.checkBox} ${styles.checkMember}`}>
                <IconCheck size={10} />
              </div>
            ) : (
              <div className={`${checkStyles.checkBox} ${isExcluded ? checkStyles.checkBoxExcluded : isSelected ? (filterSelection ? checkStyles.checkBoxFilterChecked : styles.checkSelected) : ''}`}>
                {isExcluded ? <IconX size={10} /> : isSelected ? <IconCheck size={10} /> : null}
              </div>
            )}
          </div>
        )}
        <IconFolder size={20} stroke={1.5} className={styles.folderIcon} style={node.color ? { color: node.color } : undefined} />
        <span className={styles.name}>
          {searchLower ? highlightMatch(node.name, searchLower) : node.name}
        </span>
        <span className={styles.right}>
          {isMember ? <span className={styles.memberBadge}>{t("Member")}</span> : parentName ? <span className={styles.parentName}>{parentName}</span> : null}
          {hasChildren ? (
            <button
              type="button"
              aria-label={isExpanded ? t("Collapse {value0}", { value0: node.name }) : t("Expand {value0}", { value0: node.name })}
              className={`${styles.expandBtn} ${isExpanded ? styles.expandBtnExpanded : ''}`}
              onClick={(e) => { e.stopPropagation(); toggleExpand(node.id); }}
            >
              <IconChevronRight size={12} />
            </button>
          ) : <span className={styles.expandPlaceholder} />}
        </span>
      </div>,
    ];

    if (isExpanded) {
      const childAncestors = depth > 0
        ? [...ancestorContinues, !isLastChild]
        : ancestorContinues;
      children.forEach((child, index) => {
        result.push(...renderNode(child, childAncestors, index === children.length - 1));
      });
    }

    return result;
  }

  if (tree.length === 0) return <div className={styles.empty}>{t("No folders")}</div>;

  return (
    <div className={styles.tree}>
      {tree.flatMap((root, index) => renderNode(root, [], index === tree.length - 1))}
    </div>
  );
}
