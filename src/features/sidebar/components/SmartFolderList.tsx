import { useCallback, useMemo, useState } from 'react';
import { useDisclosure } from '@mantine/hooks';
import { useInlineRename } from '../../../shared/hooks/useInlineRename';
import { api } from '#desktop/api';
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
import { registerUndoAction } from '../../../shared/controllers/undoRedoController';
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

  const style: React.CSSProperties = {
    marginLeft: node.depth * 20,
    opacity: isDragging ? 0.3 : 1,
    position: 'relative',
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

function parseSmartFolderId(id: string | null | undefined): number | null {
  if (!id) return null;
  const parsed = parseInt(id, 10);
  return Number.isNaN(parsed) ? null : parsed;
}

export function SmartFolderList({ onFolderUpdated }: SmartFolderListProps) {
  const { smartFolders: domainFolders, smartFolderCounts: counts } = useDomainStore();
  const { activeSmartFolderId, navigateToSmartFolder, navigateTo } = useNavigationStore();

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

  const refreshSidebarAndGrid = useCallback(async () => {
    useDomainStore.getState().invalidate();
    await onFolderUpdated?.();
  }, [onFolderUpdated]);

  const updateFolder = useCallback(async (
    folder: SmartFolder,
    updates: Partial<SmartFolder>,
    options?: { recordUndo?: boolean },
  ) => {
    if (!folder.id) return;
    try {
      const updated = { ...folder, ...updates };
      await api.smartFolders.update(folder.id, folderToRust(updated));
      if (options?.recordUndo !== false) {
        const before = { ...folder };
        registerUndoAction({
          label: 'Update smart folder',
          undo: async () => {
            await updateFolder(updated, before, { recordUndo: false });
          },
          redo: async () => {
            await updateFolder(before, updates, { recordUndo: false });
          },
        });
      }
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

  const buildSiblingMovesForParent = useCallback((parentId: string | null): [number, number][] => {
    const siblings = folders
      .filter((folder) => {
        const folderParent = folder.parent_id != null ? String(folder.parent_id) : null;
        return folderParent === parentId;
      })
      .sort((a, b) => {
        const aOrder = a.display_order ?? Number.MAX_SAFE_INTEGER;
        const bOrder = b.display_order ?? Number.MAX_SAFE_INTEGER;
        if (aOrder !== bOrder) return aOrder - bOrder;
        return a.id.localeCompare(b.id, undefined, { numeric: true });
      });
    return siblings.map((folder, index) => [parseInt(folder.id, 10), (index + 1) * 1000]);
  }, [folders]);

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
    const oldParentId = draggedNode.parent_id ? parseInt(draggedNode.parent_id, 10) : null;
    const oldSiblingMoves = buildSiblingMovesForParent(draggedNode.parent_id ?? null);

    let redoParentId: number | null = oldParentId;
    let redoSiblingMoves: [number, number][] = [];

    try {
      if (indicator.position === 'inside') {
        const newParentId = parseInt(targetNode.id, 10);
        const siblingNodes = folders
          .filter((folder) => folder.parent_id === newParentId && folder.id !== draggedNode.id)
          .sort((a, b) => (a.display_order ?? 0) - (b.display_order ?? 0));
        const reordered = [...siblingNodes, folders.find((folder) => folder.id === draggedNode.id)!];
        redoParentId = newParentId;
        redoSiblingMoves = reordered.map((folder, index) => [parseInt(folder.id, 10), (index + 1) * 1000]);
        await api.smartFolders.move(draggedId, newParentId, redoSiblingMoves);
        setCollapsedNodes((prev) => {
          const next = new Set(prev);
          next.delete(targetNode.id);
          return next;
        });
      } else {
        const targetParentId = targetNode.parent_id ? parseInt(targetNode.parent_id, 10) : null;
        const siblingNodes = folders
          .filter((folder) => folder.parent_id === targetParentId && folder.id !== draggedNode.id)
          .sort((a, b) => (a.display_order ?? 0) - (b.display_order ?? 0));
        const targetIdx = siblingNodes.findIndex((folder) => folder.id === targetNode.id);
        const insertIdx = indicator.position === 'before' ? targetIdx : targetIdx + 1;
        const reordered = [...siblingNodes];
        reordered.splice(insertIdx, 0, folders.find((folder) => folder.id === draggedNode.id)!);
        redoParentId = targetParentId;
        redoSiblingMoves = reordered.map((folder, index) => [parseInt(folder.id, 10), (index + 1) * 1000]);
        await api.smartFolders.move(draggedId, targetParentId, redoSiblingMoves);
      }

      registerUndoAction({
          label: 'Move smart folder',
          undo: async () => {
            await api.smartFolders.move(draggedId, oldParentId, oldSiblingMoves);
            await refreshSidebarAndGrid();
          },
          redo: async () => {
            await api.smartFolders.move(draggedId, redoParentId, redoSiblingMoves);
            await refreshSidebarAndGrid();
          },
        });
      await refreshSidebarAndGrid();
    } catch (error) {
      console.error('Smart folder DnD failed:', error);
    }
  }, [buildSiblingMovesForParent, dropIndicator, folders, nodeMap, refreshSidebarAndGrid]);

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
          let created = await api.smartFolders.create(folderToRust({
            ...smartFolder,
            id: undefined,
            name: `${smartFolder.name} (copy)`,
          }));
          registerUndoAction({
            label: 'Duplicate smart folder',
            undo: async () => {
              if (created?.id) await api.smartFolders.delete(created.id);
              await refreshSidebarAndGrid();
            },
            redo: async () => {
              created = await api.smartFolders.create(folderToRust({
                ...smartFolder,
                id: undefined,
                name: `${smartFolder.name} (copy)`,
              }));
              await refreshSidebarAndGrid();
            },
          });
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
        if (!folder.id) return;
        try {
          const snapshot = { ...smartFolder };
          const childMoves = node.children.map((child, index) => [parseInt(child.id, 10), (index + 1) * 1000] as [number, number]);
          await api.smartFolders.delete(folder.id);
          let recreated: SmartFolder | null = null;
          registerUndoAction({
            label: 'Delete smart folder',
            undo: async () => {
              recreated = await api.smartFolders.create(folderToRust({ ...snapshot, id: undefined }));
              if (recreated?.id && childMoves.length > 0) {
                const newParentId = parseInt(recreated.id, 10);
                await api.smartFolders.move(childMoves[0][0], newParentId, childMoves);
              }
              await refreshSidebarAndGrid();
            },
            redo: async () => {
              const id = recreated?.id ?? snapshot.id;
              if (id) await api.smartFolders.delete(id);
              await refreshSidebarAndGrid();
            },
          });
          await refreshSidebarAndGrid();
          if (activeSmartFolderId === folder.id) navigateTo('images');
        } catch (error) {
          console.error('Delete failed:', error);
        }
      },
    });
    contextMenu.open(e, items);
  }, [activeSmartFolderId, contextMenu, folders, navigateTo, onFolderUpdated, openCreateChild, openModal, startRename, updateFolder]);

  const activeFolder = activeId ? flatNodes.find((node) => node.id === activeId) : null;
  const sortableIds = flatNodes.map((node) => node.id);

  return (
    <>
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
                    onClick={() => {
                      if (isRenaming) return;
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
