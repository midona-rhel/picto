/**
 * FolderPickerModal — wide GlassModal for managing folder membership.
 *
 * Sidebar: Recent | Members | All (non-collapsible, with separators).
 * Supports both adding to and removing from folders.
 * No icons on sidebar items. No collapse/toggle buttons.
 */

import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import { IconSearch, IconFolder, IconCheck, IconMinus } from '@tabler/icons-react';
import { GlassModal } from '../../shared/ui/GlassModal';
import { buildTree } from '../../shared/ui/FolderTree';
import { DynamicIcon } from '../../shared/ui/DynamicIcon';
import { folderPickerModalAtom } from '../../state/modals';
import { selectionCountAtom, selectionTargetAtom } from '../../state/selection';
import { displayedInspectorItemDetailsAtom } from '../../state/inspector';
import { folderNodesAtom } from '../../state/sidebar';
import { useRecentFolders } from '../../shared/hooks/useRecentFolders';
import * as entityMutations from '../../controllers/entityMutations';
import type { SidebarNodeDto } from '../../shared/types/canonical';
import btnStyles from '../../shared/styles/actionButton.module.css';
import checkStyles from '../../shared/ui/OverlayShell/OverlayShell.module.css';
import styles from './FolderPickerModal.module.css';
import { t } from '../../i18n';

type SidebarMode = 'recent' | 'members' | 'all';

export function FolderPickerModal() {
  const modalState = useAtomValue(folderPickerModalAtom);
  const setModalState = useSetAtom(folderPickerModalAtom);
  const target = useAtomValue(selectionTargetAtom);
  const selectionCount = useAtomValue(selectionCountAtom);
  const folderNodes = useAtomValue(folderNodesAtom);
  const entityData = useAtomValue(displayedInspectorItemDetailsAtom);
  const open = modalState.open;
  const close = useCallback(() => setModalState({ open: false }), [setModalState]);

  const [recentFolderIds] = useRecentFolders(15);
  const [query, setQuery] = useState('');
  const [checked, setChecked] = useState<Set<number>>(new Set());
  const [unchecked, setUnchecked] = useState<Set<number>>(new Set());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [sidebarMode, setSidebarMode] = useState<SidebarMode>('all');
  const searchRef = useRef<HTMLInputElement>(null);

  const memberOf = useMemo(() => {
    return new Set(entityData?.folder_ids ?? []);
  }, [entityData]);

  const tree = useMemo(() => buildTree(folderNodes), [folderNodes]);
  const searchLower = query.trim().toLowerCase();

  // Recent folders — resolve IDs to node data
  const recentFolders = useMemo(() => {
    const nodeById = new Map(folderNodes.map((n) => [parseInt(n.id.slice(7), 10), n]));
    return recentFolderIds
      .map((id) => nodeById.get(id))
      .filter((n): n is NonNullable<typeof n> => n != null);
  }, [recentFolderIds, folderNodes]);

  // Member folders
  const memberFolders = useMemo(() => {
    return folderNodes.filter((n) => memberOf.has(parseInt(n.id.slice(7), 10)));
  }, [folderNodes, memberOf]);

  useEffect(() => {
    const ids = new Set<string>();
    (function walk(nodes) { for (const n of nodes) { ids.add(n.node.id); walk(n.children); } })(tree);
    setExpanded(ids);
  }, [tree]);

  useEffect(() => {
    if (open) {
      setQuery('');
      setChecked(new Set());
      setUnchecked(new Set());
      setSidebarMode('all');
      setTimeout(() => searchRef.current?.focus(), 50);
    }
  }, [open]);

  const toggleFolder = useCallback((folderId: number) => {
    const isMember = memberOf.has(folderId);
    if (isMember) {
      setUnchecked((prev) => {
        const next = new Set(prev);
        if (next.has(folderId)) next.delete(folderId); else next.add(folderId);
        return next;
      });
      setChecked((prev) => { const next = new Set(prev); next.delete(folderId); return next; });
    } else {
      setChecked((prev) => {
        const next = new Set(prev);
        if (next.has(folderId)) next.delete(folderId); else next.add(folderId);
        return next;
      });
    }
  }, [memberOf]);

  const applyFolders = useCallback(async () => {
    if (!target || (checked.size === 0 && unchecked.size === 0)) return;
    await Promise.all([
      ...[...checked].map((folderId) => entityMutations.updateTargetFolderMembership(target, folderId, 'add')),
      ...[...unchecked].map((folderId) => entityMutations.updateTargetFolderMembership(target, folderId, 'remove')),
    ]);
    entityMutations.settleSelectionAfterMutation();
    close();
    setChecked(new Set());
    setUnchecked(new Set());
  }, [target, checked, unchecked, close]);

  const toggleExpand = useCallback((nodeId: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(nodeId)) next.delete(nodeId); else next.add(nodeId);
      return next;
    });
  }, []);

  // Display folders based on sidebar mode
  const displayFolders = useMemo(() => {
    if (sidebarMode === 'recent') return recentFolders;
    if (sidebarMode === 'members') return memberFolders;
    return null; // "all" uses the tree
  }, [sidebarMode, recentFolders, memberFolders]);

  const pendingCount = checked.size + unchecked.size;
  const summaryParts: string[] = [];
  if (checked.size > 0) summaryParts.push(`+${checked.size}`);
  if (unchecked.size > 0) summaryParts.push(`−${unchecked.size}`);
  const summaryText = summaryParts.length > 0
    ? `${summaryParts.join(', ')}${selectionCount > 0 ? ` · ${selectionCount} file${selectionCount !== 1 ? 's' : ''}` : ''}`
    : (selectionCount > 0 ? `${selectionCount} file${selectionCount !== 1 ? 's' : ''} selected` : '');

  return (
    <GlassModal
      open={open}
      onClose={close}
      title={t("Folders")}
      size="md"
      flush
      footer={
        <>
          <span className={styles.footerLeft}>
            {summaryText && <span className={styles.summaryText}>{summaryText}</span>}
          </span>
          <div className={btnStyles.btnGroup}>
            <button className={btnStyles.btn} onClick={close} type="button">{t("Cancel")}</button>
            {pendingCount > 0 && (
              <button data-modal-primary="true" className={`${btnStyles.btn} ${btnStyles.btnPrimary}`} onClick={applyFolders} type="button">
                {t("Apply (")}{pendingCount})
              </button>
            )}
          </div>
        </>
      }
    >
      <div className={styles.panelBody}>
        <div className={styles.searchBar}>
          <IconSearch size={14} className={styles.searchIcon} />
          <input
            ref={searchRef}
            className={styles.searchInput}
            placeholder={t("Search folders...")}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>

        <div className={styles.mainArea}>
          {/* Sidebar */}
          <div className={styles.sidebar}>
            <div
              className={`${styles.sidebarItem} ${sidebarMode === 'recent' ? styles.sidebarItemActive : ''}`}
              onClick={() => setSidebarMode('recent')}
            >
              <span className={styles.sidebarName}>{t("Recent")}</span>
              <span className={styles.sidebarBadge}>{recentFolders.length}</span>
            </div>
            <div
              className={`${styles.sidebarItem} ${sidebarMode === 'members' ? styles.sidebarItemActive : ''}`}
              onClick={() => setSidebarMode('members')}
            >
              <span className={styles.sidebarName}>{t("Members")}</span>
              <span className={styles.sidebarBadge}>{memberOf.size}</span>
            </div>
            <div className={styles.sidebarSep} />
            <div
              className={`${styles.sidebarItem} ${sidebarMode === 'all' ? styles.sidebarItemActive : ''}`}
              onClick={() => setSidebarMode('all')}
            >
              <span className={styles.sidebarName}>{t("All")}</span>
              <span className={styles.sidebarBadge}>{folderNodes.length}</span>
            </div>
          </div>

          {/* Content */}
          <div className={styles.content}>
            {displayFolders != null ? (
              // Flat list (recent or members)
              <div className={styles.flatList}>
                {displayFolders.length === 0 ? (
                  <div className={styles.emptyState}>
                    {sidebarMode === 'recent' ? t("No recently used folders") : t("Not a member of any folders")}
                  </div>
                ) : (
                  displayFolders.map((node) => {
                    const fid = parseInt(node.id.slice(7), 10);
                    return (
                      <FolderRow
                        key={node.id}
                        node={node}
                        folderId={fid}
                        isMember={memberOf.has(fid)}
                        isChecked={checked.has(fid)}
                        isUnchecked={unchecked.has(fid)}
                        onClick={() => toggleFolder(fid)}
                      />
                    );
                  })
                )}
              </div>
            ) : (
              // Full tree (all mode)
              <div className={styles.treeContainer}>
                {tree.length === 0 ? (
                  <div className={styles.emptyState}>{t("No folders")}</div>
                ) : (
                  tree.flatMap((root) => renderTreeNode(root, {
                    expanded, toggleExpand, searchLower, memberOf, checked, unchecked, toggleFolder,
                  }))
                )}
              </div>
            )}
          </div>
        </div>
      </div>
    </GlassModal>
  );
}

// ── Folder row ──

function FolderRow({ node, folderId: _fid, isMember, isChecked, isUnchecked, onClick, indent = 0, hasChildren, isExpanded, onToggleExpand }: {
  node: SidebarNodeDto;
  folderId: number;
  isMember: boolean;
  isChecked: boolean;
  isUnchecked: boolean;
  onClick: () => void;
  indent?: number;
  hasChildren?: boolean;
  isExpanded?: boolean;
  onToggleExpand?: () => void;
}) {
  const showChecked = (isMember && !isUnchecked) || isChecked;
  const showRemove = isMember && isUnchecked;

  return (
    <div
      className={styles.folderRow}
      style={indent > 0 ? { paddingLeft: 8 + indent * 20 } : undefined}
      onClick={onClick}
    >
      {hasChildren != null ? (
        hasChildren ? (
          <div
            className={`${styles.expandBtn} ${isExpanded ? styles.expandBtnOpen : ''}`}
            onClick={(e) => { e.stopPropagation(); onToggleExpand?.(); }}
          >
            <svg width="12" height="12" viewBox="0 0 12 12">
              <path d="M4.5 2.5L8 6L4.5 9.5" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
            </svg>
          </div>
        ) : (
          <div className={styles.expandPlaceholder} />
        )
      ) : null}
      <div className={`${checkStyles.checkBox} ${showChecked ? checkStyles.checkBoxChecked : ''} ${showRemove ? styles.checkRemove : ''}`}>
        {showChecked && <IconCheck size={10} />}
        {showRemove && <IconMinus size={10} />}
      </div>
      {node.icon ? (
        <DynamicIcon name={node.icon} size={14} color={node.color} filled />
      ) : (
        <IconFolder size={14} style={node.color ? { color: node.color } : undefined} className={styles.folderIcon} />
      )}
      <span className={styles.folderName}>{node.name}</span>
      {node.count != null && <span className={styles.folderCount}>{node.count.toLocaleString()}</span>}
    </div>
  );
}

// ── Tree rendering ──

interface TreeCtx {
  expanded: Set<string>;
  toggleExpand: (nodeId: string) => void;
  searchLower: string;
  memberOf: Set<number>;
  checked: Set<number>;
  unchecked: Set<number>;
  toggleFolder: (folderId: number) => void;
}

interface TreeNode {
  node: SidebarNodeDto;
  folderId: number;
  children: TreeNode[];
  depth: number;
}

function matchesSearch(treeNode: TreeNode, q: string): boolean {
  if (treeNode.node.name.toLowerCase().includes(q)) return true;
  return treeNode.children.some((c) => matchesSearch(c, q));
}

function renderTreeNode(treeNode: TreeNode, ctx: TreeCtx): React.ReactNode[] {
  const { node, folderId, children, depth } = treeNode;
  const hasChildren = children.length > 0;
  const isExpanded = ctx.expanded.has(node.id);

  if (ctx.searchLower && !matchesSearch(treeNode, ctx.searchLower)) return [];

  const result: React.ReactNode[] = [
    <FolderRow
      key={node.id}
      node={node}
      folderId={folderId}
      isMember={ctx.memberOf.has(folderId)}
      isChecked={ctx.checked.has(folderId)}
      isUnchecked={ctx.unchecked.has(folderId)}
      onClick={() => ctx.toggleFolder(folderId)}
      indent={depth}
      hasChildren={hasChildren}
      isExpanded={isExpanded}
      onToggleExpand={() => ctx.toggleExpand(node.id)}
    />,
  ];

  if (isExpanded) {
    for (const child of children) result.push(...renderTreeNode(child, ctx));
  }

  return result;
}
