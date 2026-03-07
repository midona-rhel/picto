
import { useCallback, useMemo, useState, useRef } from 'react';
import { IconTag } from '@tabler/icons-react';
import {
  DndContext,
  DragOverlay,
} from '@dnd-kit/core';
import { useSortable, SortableContext, verticalListSortingStrategy } from '@dnd-kit/sortable';

import { useDomainStore } from '../../../state/domainStore';
import { useNavigationStore } from '../../../state/navigationStore';
import { ContextMenu, useContextMenu, type ContextMenuEntry } from '../../../shared/components/ContextMenu';
import {
  buildFolderMultiMenu,
  buildFolderSingleMenu,
} from '../../../shared/components/context-actions/folderActions';
import { DynamicIcon } from '../../smart-folders/components/iconRegistry';
import { useImageDragDropTarget } from '../../../shared/lib/imageDrag';
import {
  type TreeNode,
  type DropIndicator,
  buildFolderTree,
  parseFolderId,
  getFolderAutoTags,
} from '../lib/folderTreeData';
import { useFolderTreeActions } from '../hooks/useFolderTreeActions';
import { useFolderTreeDnd } from '../hooks/useFolderTreeDnd';
import { SidebarSection } from './SidebarSection';
import { SidebarItem } from './SidebarItem';
import styles from './Sidebar.module.css';

/** Sortable folder row wrapper */
function SortableFolderRow({
  node,
  children,
  dropIndicator,
}: {
  node: TreeNode;
  children: React.ReactNode;
  dropIndicator: DropIndicator | null;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    isDragging,
  } = useSortable({ id: node.id });

  const style: React.CSSProperties = {
    marginLeft: node.depth * 20,
    opacity: isDragging ? 0.3 : 1,
    position: 'relative' as const,
  };

  const isDropBefore = dropIndicator?.nodeId === node.id && dropIndicator.position === 'before';
  const isDropInside = dropIndicator?.nodeId === node.id && dropIndicator.position === 'inside';
  const isDropAfter = dropIndicator?.nodeId === node.id && dropIndicator.position === 'after';

  return (
    <div ref={setNodeRef} style={style} {...attributes} {...listeners} className={styles.folderRow}>
      {isDropBefore && <div className={styles.dropLine} style={{ top: 0 }} />}
      <div className={isDropInside ? styles.dropHighlight : undefined}>
        {children}
      </div>
      {isDropAfter && <div className={styles.dropLine} style={{ bottom: 0 }} />}
    </div>
  );
}

export function FolderTree() {
  const folderNodes = useDomainStore((s) => s.folderNodes);
  const { activeFolder, navigateToFolder, setActiveFolder } = useNavigationStore();

  const tree = buildFolderTree(folderNodes);

  const [collapsedNodes, setCollapsedNodes] = useState<Set<string>>(new Set());
  const toggleExpand = useCallback((nodeId: string) => {
    setCollapsedNodes((prev) => {
      const next = new Set(prev);
      if (next.has(nodeId)) next.delete(nodeId); else next.add(nodeId);
      return next;
    });
  }, []);

  const flat: TreeNode[] = [];
  const walk = (nodes: TreeNode[]) => {
    for (const n of nodes) {
      flat.push(n);
      if (n.children.length > 0 && !collapsedNodes.has(n.id)) {
        walk(n.children);
      }
    }
  };
  walk(tree);

  const nodeMap = useMemo(() => {
    const map = new Map<string, TreeNode>();
    const walkAll = (nodes: TreeNode[]) => {
      for (const n of nodes) { map.set(n.id, n); walkAll(n.children); }
    };
    walkAll(tree);
    return map;
  }, [tree]);

  const expandFolder = useCallback((nodeId: string) => {
    setCollapsedNodes((prev) => { const next = new Set(prev); next.delete(nodeId); return next; });
  }, []);

  // ── Extracted hooks ──
  const actions = useFolderTreeActions({
    folderNodes,
    nodeMap,
    activeFolder,
    setActiveFolder,
    expandFolder,
    setCollapsedNodes,
  });

  const dnd = useFolderTreeDnd({
    nodeMap,
    folderNodes,
    setCollapsedNodes,
  });

  // ── Selection ──
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const lastClickedId = useRef<string | null>(null);

  const handleFolderClick = useCallback((e: React.MouseEvent, node: TreeNode) => {
    const folderId = parseFolderId(node.id);
    if (folderId == null) return;

    if (e.metaKey || e.ctrlKey) {
      setSelectedIds((prev) => {
        const next = new Set(prev);
        if (next.has(node.id)) next.delete(node.id); else next.add(node.id);
        return next;
      });
      lastClickedId.current = node.id;
    } else if (e.shiftKey && lastClickedId.current) {
      const startIdx = flat.findIndex((n) => n.id === lastClickedId.current);
      const endIdx = flat.findIndex((n) => n.id === node.id);
      if (startIdx !== -1 && endIdx !== -1) {
        const lo = Math.min(startIdx, endIdx);
        const hi = Math.max(startIdx, endIdx);
        setSelectedIds((prev) => {
          const next = new Set(prev);
          for (let i = lo; i <= hi; i++) next.add(flat[i].id);
          return next;
        });
      }
    } else {
      setSelectedIds(new Set());
      lastClickedId.current = node.id;
      navigateToFolder({ folder_id: folderId, name: node.name });
    }
  }, [flat, navigateToFolder]);

  // ── Expand/collapse helpers ──
  const expandSameLevel = useCallback((node: TreeNode) => {
    const siblings = folderNodes.filter((n) => n.parent_id === node.parent_id);
    setCollapsedNodes((prev) => {
      const next = new Set(prev);
      for (const s of siblings) next.delete(s.id);
      return next;
    });
  }, [folderNodes]);

  const expandAll = useCallback(() => { setCollapsedNodes(new Set()); }, []);

  const collapseAll = useCallback(() => {
    const allIds = new Set(folderNodes.filter((n) => {
      return folderNodes.some((c) => c.parent_id === n.id);
    }).map((n) => n.id));
    setCollapsedNodes(allIds);
  }, [folderNodes]);

  const toggleSameLevelFolders = useCallback((node: TreeNode) => {
    const siblings = folderNodes.filter((entry) => entry.parent_id === node.parent_id);
    const anyCollapsed = siblings.some((entry) => collapsedNodes.has(entry.id));
    if (anyCollapsed) { expandSameLevel(node); return; }
    setCollapsedNodes((prev) => {
      const next = new Set(prev);
      for (const sibling of siblings) {
        if (folderNodes.some((child) => child.parent_id === sibling.id)) next.add(sibling.id);
      }
      return next;
    });
  }, [collapsedNodes, expandSameLevel, folderNodes]);

  // ── Context menu ──
  const contextMenu = useContextMenu();
  const [contextMenuNodeId, setContextMenuNodeId] = useState<string | null>(null);
  const dragDropTarget = useImageDragDropTarget();

  const handleContextMenu = useCallback((e: React.MouseEvent, node: TreeNode) => {
    setContextMenuNodeId(node.id);
    const folderId = parseFolderId(node.id);
    if (folderId == null) return;
    const isMulti = selectedIds.size > 1 && selectedIds.has(node.id);

    if (isMulti) {
      const ids = [...selectedIds].map(parseFolderId).filter((id): id is number => id != null);
      const items = buildFolderMultiMenu({
        sortBy: {
          currentLevelAsc: () => actions.handleSortFolders(node.parent_id, 'asc'),
          currentLevelDesc: () => actions.handleSortFolders(node.parent_id, 'desc'),
          allLevelsAsc: () => actions.handleSortAllFolders('asc'),
          allLevelsDesc: () => actions.handleSortAllFolders('desc'),
        },
        iconAndColor: {
          onIconChange: (icon) => actions.applyIconToFolders(ids, icon),
          onColorChange: (color) => actions.applyColorToFolders(ids, color),
          iconLabel: 'Change Icon...',
        },
        deleteFolders: () => actions.handleBatchDelete(selectedIds),
        deleteLabel: `Remove ${selectedIds.size} Folders`,
      });
      contextMenu.open(e, items);
      return;
    }

    const hasChildren = node.children.length > 0;
    const autoTags = getFolderAutoTags(node);
    const items: ContextMenuEntry[] = buildFolderSingleMenu({
      createFolder: () => actions.createSiblingFolder(node),
      createSubfolder: () => actions.createSubfolderForNode(node, folderId),
      renameFolder: () => actions.startRename(node.id, node.name),
      setAutoTags: () => actions.openFolderAutoTagsEditor(folderId, node.name, autoTags),
      sortBy: {
        currentLevelAsc: () => actions.handleSortFolders(node.parent_id, 'asc'),
        currentLevelDesc: () => actions.handleSortFolders(node.parent_id, 'desc'),
        allLevelsAsc: () => actions.handleSortAllFolders('asc'),
        allLevelsDesc: () => actions.handleSortAllFolders('desc'),
      },
      expandActions: {
        toggleFolder: hasChildren ? () => toggleExpand(node.id) : undefined,
        toggleSameLevel: () => toggleSameLevelFolders(node),
        toggleAll: () => {
          if (collapsedNodes.size > 0) expandAll();
          else collapseAll();
        },
      },
      iconAndColor: {
        iconValue: node.icon ?? null,
        colorValue: node.color ?? null,
        onIconChange: (icon) => actions.applyIconToFolders([folderId], icon),
        onColorChange: (color) => actions.applyColorToFolders([folderId], color),
        iconLabel: 'Change Icon...',
      },
      deleteFolder: () => actions.handleDelete(node.id),
      deleteLabel: 'Remove Folder',
      showDuplicate: true,
      showExport: true,
    });
    contextMenu.open(e, items);
  }, [actions, collapsedNodes.size, collapseAll, contextMenu, expandAll, selectedIds, toggleExpand, toggleSameLevelFolders]);

  // ── Render ──
  const sortableIds = flat.map((n) => n.id);
  const activeDragNode = dnd.activeId ? nodeMap.get(dnd.activeId) : null;

  return (
    <>
      <SidebarSection title="Folders" onAdd={actions.handleCreate}>
        <DndContext
          sensors={dnd.sensors}
          onDragStart={dnd.handleDragStart}
          onDragMove={dnd.handleDragMove}
          onDragEnd={dnd.handleDragEnd}
          onDragCancel={dnd.handleDragCancel}
        >
          <SortableContext items={sortableIds} strategy={verticalListSortingStrategy}>
            {flat.map((node) => {
              const folderId = parseFolderId(node.id);
              const isActive = folderId != null && activeFolder?.folder_id === folderId;
              const isRenaming = actions.renamingId === node.id;
              const hasChildren = node.children.length > 0;
              const isExpanded = !collapsedNodes.has(node.id);
              const autoTags = getFolderAutoTags(node);

              return (
                <SortableFolderRow key={node.id} node={node} dropIndicator={dnd.dropIndicator}>
                  {hasChildren && (
                    <span
                      className={styles.folderArrow}
                      onClick={(e) => { e.stopPropagation(); toggleExpand(node.id); }}
                    >
                      <span className={`${styles.folderTriangle} ${!isExpanded ? styles.folderTriangleCollapsed : styles.folderTriangleExpanded}`} />
                    </span>
                  )}

                  <SidebarItem
                    icon={<DynamicIcon name={node.icon ?? (isExpanded ? 'IconFolderOpen' : 'IconFolder')} size={18} color={node.color ?? 'currentColor'} />}
                    label={isRenaming ? undefined : node.name}
                    count={isRenaming ? null : node.count}
                    isActive={isActive}
                    isSelected={selectedIds.has(node.id)}
                    isDropTarget={folderId != null && dragDropTarget === folderId}
                    isContextHighlight={contextMenuNodeId === node.id && !isActive}
                    onClick={(e) => { if (!isRenaming) handleFolderClick(e, node); }}
                    onContextMenu={(e) => handleContextMenu(e, node)}
                    onNativeDrop={actions.handleFilesDropOnFolder}
                    dataFolderDropId={folderId}
                  >
                    {isRenaming ? (
                      <input
                        ref={actions.renameInputRef}
                        className={styles.renameInput}
                        value={actions.renameValue}
                        onChange={(e) => actions.setRenameValue(e.target.value)}
                        onBlur={actions.commitRename}
                        onKeyDown={actions.renameKeyHandler}
                        onClick={(e) => e.stopPropagation()}
                      />
                    ) : (
                      <span className={styles.itemLabelRow}>
                        <span className={styles.itemLabel}>{node.name}</span>
                        {autoTags.length > 0 ? (
                          <span className={styles.folderAutoTagIndicator} title={`${autoTags.length} auto-tag${autoTags.length === 1 ? '' : 's'}`}>
                            <IconTag size={11} />
                          </span>
                        ) : null}
                      </span>
                    )}
                  </SidebarItem>
                </SortableFolderRow>
              );
            })}
          </SortableContext>

          <DragOverlay dropAnimation={null}>
            {activeDragNode ? (
              <div className={styles.dragOverlay}>
                <SidebarItem
                  icon={<DynamicIcon name={activeDragNode.icon ?? 'IconFolder'} size={18} color={activeDragNode.color ?? 'currentColor'} />}
                  label={activeDragNode.name}
                />
              </div>
            ) : null}
          </DragOverlay>
        </DndContext>
      </SidebarSection>

      {contextMenu.state && (
        <ContextMenu
          items={contextMenu.state.items}
          position={contextMenu.state.position}
          onClose={() => { contextMenu.close(); setContextMenuNodeId(null); }}
        />
      )}
    </>
  );
}
