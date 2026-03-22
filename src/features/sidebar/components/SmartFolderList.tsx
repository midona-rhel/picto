import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useDisclosure } from '@mantine/hooks';
import { useInlineRename } from '../../../shared/hooks/useInlineRename';
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragMoveEvent,
  type DragStartEvent,
} from '@dnd-kit/core';
import { SortableContext, useSortable, verticalListSortingStrategy } from '@dnd-kit/sortable';

import { ContextMenu, useContextMenu } from '../../../shared/components/ContextMenu';
import { SmartFolderModal } from '../../smart-folders/components/SmartFolderModal';
import { DynamicIcon, DEFAULT_FOLDER_ICON } from '../../smart-folders/components/iconRegistry';
import type { SmartFolder } from '../../smart-folders/components/types';
import { folderToRust } from '../../smart-folders/components/types';
import { smartFoldersController } from '../../../controllers/smartFoldersController';
import { useDomainStore } from '../../../state/domainStore';
import { useNavigationStore } from '../../../state/navigationStore';
import { SidebarSection } from './SidebarSection';
import { SidebarItem } from './SidebarItem';
import { buildSmartFolderItemMenu } from '../../../shared/components/context-actions/smartFolderActions';
import {
  type SmartFolderDropIndicator,
  type SmartFolderDropPosition,
  type SmartFolderTreeNode,
  buildSmartFolderTree,
  collectSmartFolderDescendantIds,
} from '../lib/smartFolderTreeData';
import styles from './Sidebar.module.css';

interface SmartFolderListProps {
  onFolderUpdated?: () => void | Promise<void>;
}

function SortableSmartFolderRow({
  node,
  children,
  dropIndicator,
}: {
  node: SmartFolderTreeNode;
  children: React.ReactNode;
  dropIndicator: SmartFolderDropIndicator | null;
}) {
  const {
    attributes,
    listeners,
    setNodeRef,
    isDragging,
  } = useSortable({ id: node.id });

  // Wrap dnd-kit listeners to skip drag initiation when modifier keys are held
  // so shift-click and ctrl/cmd-click work for selection instead.
  const filteredListeners = useMemo(() => {
    if (!listeners) return listeners;
    const wrapped: Record<string, (e: React.PointerEvent) => void> = {};
    for (const [key, handler] of Object.entries(listeners)) {
      wrapped[key] = (e: React.PointerEvent) => {
        if (e.shiftKey || e.metaKey || e.ctrlKey) return;
        (handler as (e: React.PointerEvent) => void)(e);
      };
    }
    return wrapped;
  }, [listeners]);

  const style: React.CSSProperties = {
    marginLeft: node.depth * 20,
    opacity: isDragging ? 0.3 : 1,
    position: 'relative',
  };

  const isDropBefore = dropIndicator?.nodeId === node.id && dropIndicator.position === 'before';
  const isDropInside = dropIndicator?.nodeId === node.id && dropIndicator.position === 'inside';
  const isDropAfter = dropIndicator?.nodeId === node.id && dropIndicator.position === 'after';

  return (
    <div ref={setNodeRef} style={style} {...attributes} {...filteredListeners} className={styles.folderRow}>
      {isDropBefore && <div className={styles.dropLine} style={{ top: 0 }} />}
      <div className={isDropInside ? styles.dropHighlight : undefined}>
        {children}
      </div>
      {isDropAfter && <div className={styles.dropLine} style={{ bottom: 0 }} />}
    </div>
  );
}

function parseSmartFolderId(id: string | null | undefined): number | null {
  if (!id) return null;
  const parsed = parseInt(id, 10);
  return Number.isNaN(parsed) ? null : parsed;
}

export function SmartFolderList({ onFolderUpdated }: SmartFolderListProps) {
  const { smartFolders: domainFolders, smartFolderCounts: counts } = useDomainStore();
  const { activeSmartFolderId, navigateToSmartFolder } = useNavigationStore();

  const folders = useMemo(() => domainFolders.map((sf) => ({
    id: sf.id,
    name: sf.name,
    parent_id: parseSmartFolderId(sf.parent_id),
    icon: sf.icon ?? undefined,
    color: sf.color ?? undefined,
    predicate: sf.localPredicate ?? sf.predicate ?? { groups: [] },
    sort_field: sf.sort_field ?? undefined,
    sort_order: sf.sort_order ?? undefined,
    display_order: sf.display_order ?? undefined,
    count: sf.count,
    freshness: sf.freshness,
    effectivePredicate: sf.predicate,
    hasEffectiveRules: sf.hasEffectiveRules,
    hasLocalRules: sf.hasLocalRules,
  })), [domainFolders]);

  const tree = useMemo(
    () => buildSmartFolderTree(
      folders.map((folder) => ({
        id: folder.id,
        name: folder.name,
        parent_id: folder.parent_id != null ? String(folder.parent_id) : null,
        display_order: folder.display_order ?? null,
        icon: folder.icon ?? null,
        color: folder.color ?? null,
        count: folder.count,
        freshness: folder.freshness,
        predicate: folder.effectivePredicate,
        localPredicate: folder.predicate,
        hasEffectiveRules: folder.hasEffectiveRules,
        hasLocalRules: folder.hasLocalRules,
        sort_field: folder.sort_field ?? null,
        sort_order: folder.sort_order ?? null,
      })),
    ),
    [folders],
  );

  const [collapsedNodes, setCollapsedNodes] = useState<Set<string>>(new Set());
  const toggleExpand = useCallback((nodeId: string) => {
    setCollapsedNodes((prev) => {
      const next = new Set(prev);
      if (next.has(nodeId)) next.delete(nodeId);
      else next.add(nodeId);
      return next;
    });
  }, []);

  const flatNodes = useMemo(() => {
    const flat: SmartFolderTreeNode[] = [];
    const walk = (nodes: SmartFolderTreeNode[]) => {
      for (const node of nodes) {
        flat.push(node);
        if (node.children.length > 0 && !collapsedNodes.has(node.id)) {
          walk(node.children);
        }
      }
    };
    walk(tree);
    return flat;
  }, [collapsedNodes, tree]);

  const nodeMap = useMemo(() => {
    const map = new Map<string, SmartFolderTreeNode>();
    const walk = (nodes: SmartFolderTreeNode[]) => {
      for (const node of nodes) {
        map.set(node.id, node);
        walk(node.children);
      }
    };
    walk(tree);
    return map;
  }, [tree]);

  const [modalOpen, { open: openModal, close: closeModal }] = useDisclosure(false);
  const [editingFolder, setEditingFolder] = useState<SmartFolder | null>(null);
  const [initialParentId, setInitialParentId] = useState<number | null>(null);
  const contextMenu = useContextMenu();
  const [contextMenuFolderId, setContextMenuFolderId] = useState<string | null>(null);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const lastClickedId = useRef<string | null>(null);
  const sectionRef = useRef<HTMLDivElement>(null);

  // Clear selection when clicking anywhere outside the smart folder list
  useEffect(() => {
    if (selectedIds.size === 0) return;
    const handler = (e: MouseEvent) => {
      if (sectionRef.current && !sectionRef.current.contains(e.target as Node)) {
        setSelectedIds(new Set());
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [selectedIds.size]);

  const refreshSidebarAndGrid = useCallback(async () => {
    await onFolderUpdated?.();
  }, [onFolderUpdated]);

  const updateFolder = useCallback(async (
    folder: SmartFolder,
    updates: Partial<SmartFolder>,
  ) => {
    if (!folder.id) return;
    try {
      const updated = { ...folder, ...updates };
      await smartFoldersController.update(folder.id, folderToRust(updated), folderToRust(folder));
      await refreshSidebarAndGrid();
    } catch (e) {
      console.error('Update failed:', e);
    }
  }, [refreshSidebarAndGrid]);

  const handleRenameCommit = useCallback(async (id: string, newName: string) => {
    const folder = folders.find((item) => item.id === id);
    if (!folder) return;
    await updateFolder({
      id: folder.id,
      name: folder.name,
      parent_id: folder.parent_id ?? null,
      icon: folder.icon ?? null,
      color: folder.color ?? null,
      predicate: folder.predicate,
      sort_field: folder.sort_field ?? null,
      sort_order: folder.sort_order ?? null,
    }, { name: newName });
  }, [folders, updateFolder]);

  const {
    renamingId: renamingFolderId, renameValue, startRename, setRenameValue,
    commitRename, renameInputRef, renameKeyHandler,
  } = useInlineRename(handleRenameCommit);

  const openCreateRoot = useCallback(() => {
    setEditingFolder(null);
    setInitialParentId(null);
    openModal();
  }, [openModal]);

  const openCreateChild = useCallback((parentId: number) => {
    setEditingFolder(null);
    setInitialParentId(parentId);
    openModal();
  }, [openModal]);


  const [activeId, setActiveId] = useState<string | null>(null);
  const [dropIndicator, setDropIndicator] = useState<SmartFolderDropIndicator | null>(null);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
  );

  const handleDragStart = useCallback((event: DragStartEvent) => {
    setActiveId(String(event.active.id));
  }, []);

  const handleDragMove = useCallback((event: DragMoveEvent) => {
    const { active, over } = event;
    if (!over || active.id === over.id) {
      setDropIndicator(null);
      return;
    }

    const draggedNode = nodeMap.get(String(active.id));
    const overNode = nodeMap.get(String(over.id));
    if (!draggedNode || !overNode) {
      setDropIndicator(null);
      return;
    }

    const descendants = collectSmartFolderDescendantIds(draggedNode);
    if (descendants.has(overNode.id)) {
      setDropIndicator(null);
      return;
    }

    const overRect = over.rect;
    const cursorY = event.activatorEvent instanceof MouseEvent
      ? event.activatorEvent.clientY + (event.delta?.y ?? 0)
      : overRect.top + overRect.height / 2;
    const relativeY = cursorY - overRect.top;
    const ratio = relativeY / overRect.height;

    let position: SmartFolderDropPosition;
    if (ratio < 0.25) position = 'before';
    else if (ratio > 0.75) position = 'after';
    else position = 'inside';

    setDropIndicator({ nodeId: overNode.id, position });
  }, [nodeMap]);

  const handleDragEnd = useCallback(async (event: DragEndEvent) => {
    const indicator = dropIndicator;
    setActiveId(null);
    setDropIndicator(null);

    const { active, over } = event;
    if (!over || active.id === over.id || !indicator) return;

    const draggedNode = nodeMap.get(String(active.id));
    const targetNode = nodeMap.get(indicator.nodeId);
    if (!draggedNode || !targetNode) return;

    const draggedId = parseInt(draggedNode.id, 10);
    const targetId = parseInt(targetNode.id, 10);

    try {
      // Controller owns all sibling computation, undo snapshots, and backend call
      await smartFoldersController.moveToPosition(
        draggedId,
        targetId,
        indicator.position,
        folders.map((f) => ({ id: f.id, parent_id: f.parent_id, display_order: f.display_order ?? null })),
      );
      if (indicator.position === 'inside') {
        setCollapsedNodes((prev) => { const next = new Set(prev); next.delete(targetNode.id); return next; });
      }
      await refreshSidebarAndGrid();
    } catch (error) {
      console.error('Smart folder DnD failed:', error);
    }
  }, [dropIndicator, folders, nodeMap, refreshSidebarAndGrid]);

  const handleDragCancel = useCallback(() => {
    setActiveId(null);
    setDropIndicator(null);
  }, []);

  const handleContextMenu = useCallback((e: React.MouseEvent, node: SmartFolderTreeNode) => {
    const folder = folders.find((item) => item.id === node.id);
    if (!folder) return;
    const currentSortField = folder.sort_field ?? 'date_added';
    const currentSortOrder: 'asc' | 'desc' = folder.sort_order === 'asc' ? 'asc' : 'desc';
    const smartFolder: SmartFolder = {
      id: folder.id,
      name: folder.name,
      parent_id: folder.parent_id ?? null,
      icon: folder.icon ?? null,
      color: folder.color ?? null,
      predicate: folder.predicate,
      sort_field: folder.sort_field ?? null,
      sort_order: folder.sort_order ?? null,
    };

    const items = buildSmartFolderItemMenu({
      editSmartFolder: () => {
        setEditingFolder(smartFolder);
        setInitialParentId(null);
        openModal();
      },
      createChildSmartFolder: () => {
        openCreateChild(parseInt(folder.id, 10));
      },
      renameSmartFolder: () => {
        if (folder.id) startRename(folder.id, folder.name);
      },
      setSortField: (field) => {
        void updateFolder(smartFolder, { sort_field: field });
      },
      setSortOrder: (order) => {
        void updateFolder(smartFolder, { sort_order: order });
      },
      currentSortField,
      currentSortOrder,
      duplicateSmartFolder: async () => {
        try {
          await smartFoldersController.create(
            folderToRust({ ...smartFolder, id: undefined, name: `${smartFolder.name} (copy)` }),
            'Duplicate smart folder',
          );
          await refreshSidebarAndGrid();
        } catch (error) {
          console.error('Duplicate failed:', error);
        }
      },
      iconValue: folder.icon ?? null,
      colorValue: folder.color ?? null,
      onIconChange: (icon) => {
        void updateFolder(smartFolder, { icon });
      },
      onColorChange: (color) => {
        void updateFolder(smartFolder, { color });
      },
      deleteSmartFolder: async () => {
        const currentSelectedIds = selectedIds;
        const idsToDelete = currentSelectedIds.has(node.id) && currentSelectedIds.size > 1
          ? [...currentSelectedIds]
          : folder.id ? [folder.id] : [];
        if (idsToDelete.length === 0) return;
        try {
          const snapshots = idsToDelete
            .map((id) => folders.find((f) => f.id === id))
            .filter((f): f is NonNullable<typeof f> => f != null)
            .map((f) => folderToRust({ ...f, id: undefined }));
          await smartFoldersController.deleteBatch(idsToDelete, snapshots);
          setSelectedIds(new Set());
          await refreshSidebarAndGrid();
        } catch (error) {
          console.error('Delete failed:', error);
        }
      },
    });
    contextMenu.open(e, items);
  }, [contextMenu, folders, openCreateChild, openModal, refreshSidebarAndGrid, selectedIds, startRename, updateFolder]);

  const activeFolder = activeId ? flatNodes.find((node) => node.id === activeId) : null;
  const sortableIds = flatNodes.map((node) => node.id);

  return (
    <>
      <div ref={sectionRef}>
      <SidebarSection title="Smart Folders" onAdd={openCreateRoot}>
        <DndContext
          sensors={sensors}
          onDragStart={handleDragStart}
          onDragMove={handleDragMove}
          onDragEnd={handleDragEnd}
          onDragCancel={handleDragCancel}
        >
          <SortableContext items={sortableIds} strategy={verticalListSortingStrategy}>
            {flatNodes.map((node) => {
              const folder = folders.find((item) => item.id === node.id)!;
              const isActive = activeSmartFolderId === node.id && node.hasEffectiveRules;
              const isRenaming = renamingFolderId === node.id;
              const count = node.hasEffectiveRules ? (counts[node.id] ?? node.count) : null;
              const isExpanded = !collapsedNodes.has(node.id);
              const iconName = node.icon ?? (isExpanded ? 'IconFolderOpen' : DEFAULT_FOLDER_ICON);
              const folderColor = node.color ?? 'currentColor';
              const hasChildren = node.children.length > 0;

              const labelRow = (
                <span className={styles.itemLabelRow}>
                  <span className={styles.itemLabel}>{node.name}</span>
                </span>
              );

              return (
                <SortableSmartFolderRow key={node.id} node={node} dropIndicator={dropIndicator}>
                  {hasChildren && (
                    <span
                      className={styles.folderArrow}
                      onClick={(e) => {
                        e.stopPropagation();
                        toggleExpand(node.id);
                      }}
                    >
                      <span
                        className={[
                          styles.folderTriangle,
                          !isExpanded ? styles.folderTriangleCollapsed : styles.folderTriangleExpanded,
                        ].join(' ')}
                      />
                    </span>
                  )}
                  <SidebarItem
                    icon={<DynamicIcon name={iconName} size={18} color={folderColor} />}
                    label={isRenaming ? undefined : node.name}
                    count={isRenaming ? null : count}
                    isActive={isActive}
                    isContextHighlight={contextMenuFolderId === node.id && !isActive}
                    isSelected={selectedIds.has(node.id)}
                    onClick={(e) => {
                      if (isRenaming) return;

                      // Shift-click: range select
                      if (e.shiftKey && lastClickedId.current) {
                        const startIdx = flatNodes.findIndex((n) => n.id === lastClickedId.current);
                        const endIdx = flatNodes.findIndex((n) => n.id === node.id);
                        if (startIdx !== -1 && endIdx !== -1) {
                          const lo = Math.min(startIdx, endIdx);
                          const hi = Math.max(startIdx, endIdx);
                          setSelectedIds((prev) => {
                            const next = new Set(prev);
                            for (let i = lo; i <= hi; i++) next.add(flatNodes[i].id);
                            return next;
                          });
                        }
                        return;
                      }

                      // Cmd/Ctrl-click: toggle select
                      if (e.metaKey || e.ctrlKey) {
                        setSelectedIds((prev) => {
                          const next = new Set(prev);
                          if (next.has(node.id)) next.delete(node.id);
                          else next.add(node.id);
                          return next;
                        });
                        lastClickedId.current = node.id;
                        return;
                      }

                      // Plain click: navigate + clear selection
                      setSelectedIds(new Set());
                      lastClickedId.current = node.id;

                      if (!node.hasEffectiveRules) {
                        if (hasChildren) toggleExpand(node.id);
                        return;
                      }
                      if (!isActive) {
                        navigateToSmartFolder({
                          id: folder.id,
                          name: folder.name,
                          parent_id: folder.parent_id ?? null,
                          icon: folder.icon ?? null,
                          color: folder.color ?? null,
                          predicate: node.predicate ?? { groups: [] },
                          sort_field: folder.sort_field ?? null,
                          sort_order: folder.sort_order ?? null,
                        });
                      }
                    }}
                    onContextMenu={(e) => {
                      setContextMenuFolderId(node.id);
                      handleContextMenu(e, node);
                    }}
                  >
                    {isRenaming ? (
                      <input
                        ref={renameInputRef}
                        className={styles.renameInput}
                        value={renameValue}
                        onChange={(e) => setRenameValue(e.target.value)}
                        onBlur={commitRename}
                        onKeyDown={renameKeyHandler}
                        onClick={(e) => e.stopPropagation()}
                      />
                    ) : labelRow}
                  </SidebarItem>
                </SortableSmartFolderRow>
              );
            })}
          </SortableContext>

          <DragOverlay dropAnimation={null}>
            {activeFolder ? (
              <div className={styles.dragOverlay}>
                <SidebarItem
                  icon={<DynamicIcon name={activeFolder.icon ?? DEFAULT_FOLDER_ICON} size={18} color={activeFolder.color ?? 'currentColor'} />}
                  label={activeFolder.name}
                />
              </div>
            ) : null}
          </DragOverlay>
        </DndContext>
      </SidebarSection>
      </div>

      {contextMenu.state && (
        <ContextMenu
          items={contextMenu.state.items}
          position={contextMenu.state.position}
          onClose={() => {
            contextMenu.close();
            setContextMenuFolderId(null);
          }}
        />
      )}

      <SmartFolderModal
        opened={modalOpen}
        onClose={() => {
          setInitialParentId(null);
          closeModal();
        }}
        folder={editingFolder}
        initialParentId={initialParentId}
        onSaved={async () => {
          await onFolderUpdated?.();
        }}
      />
    </>
  );
}
