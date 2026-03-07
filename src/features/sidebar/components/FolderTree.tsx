
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useInlineRename } from '../../shared/hooks/useInlineRename';
import { IconTag } from '@tabler/icons-react';
import {
  DndContext,
  PointerSensor,
  useSensor,
  useSensors,
  DragOverlay,
  type DragStartEvent,
  type DragEndEvent,
  type DragMoveEvent,
} from '@dnd-kit/core';
import { useSortable, SortableContext, verticalListSortingStrategy } from '@dnd-kit/sortable';

import { FolderController } from '../../controllers/folderController';
import { registerUndoAction } from '../../shared/controllers/undoRedoController';
import { notifyWarning } from '../../shared/lib/notify';
import { useDomainStore } from '../../state/domainStore';
import { useNavigationStore } from '../../state/navigationStore';
import { ContextMenu, useContextMenu, type ContextMenuEntry } from '../../shared/components/ContextMenu';
import {
  buildFolderMultiMenu,
  buildFolderSingleMenu,
} from '../../shared/components/context-actions/folderActions';
import { DynamicIcon } from '../smart-folders/iconRegistry';
import { imageDrag, useImageDragDropTarget } from '../../shared/lib/imageDrag';
import type { SidebarNodeDto } from '../../shared/types/sidebar';
import { TagSelectService } from '../tags/tagSelectService';
import { SidebarSection } from './SidebarSection';
import { SidebarItem } from './SidebarItem';
import styles from './Sidebar.module.css';

interface TreeNode extends SidebarNodeDto {
  children: TreeNode[];
  depth: number;
}

function buildFolderTree(nodes: SidebarNodeDto[]): TreeNode[] {
  const map = new Map<string, TreeNode>();
  for (const n of nodes) {
    map.set(n.id, { ...n, children: [], depth: 0 });
  }
  const roots: TreeNode[] = [];
  for (const [, node] of map) {
    if (node.parent_id && map.has(node.parent_id)) {
      map.get(node.parent_id)!.children.push(node);
    } else {
      roots.push(node);
    }
  }
  const sortAndSetDepth = (children: TreeNode[], depth: number) => {
    children.sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0));
    for (const n of children) {
      n.depth = depth;
      sortAndSetDepth(n.children, depth + 1);
    }
  };
  sortAndSetDepth(roots, 0);
  return roots;
}

function parseFolderId(nodeId: string): number | null {
  if (nodeId.startsWith('folder:')) {
    const num = parseInt(nodeId.slice('folder:'.length), 10);
    return isNaN(num) ? null : num;
  }
  return null;
}

function getFolderAutoTags(node: SidebarNodeDto): string[] {
  const meta = node.meta as Record<string, unknown> | null | undefined;
  const raw = meta?.auto_tags;
  return Array.isArray(raw) ? raw.filter((value): value is string => typeof value === 'string') : [];
}

/** Collect all descendant node IDs (to prevent dropping a folder into its own subtree) */
function collectDescendantIds(node: TreeNode): Set<string> {
  const ids = new Set<string>();
  const walk = (n: TreeNode) => {
    ids.add(n.id);
    for (const child of n.children) walk(child);
  };
  walk(node);
  return ids;
}

type DropPosition = 'before' | 'inside' | 'after';

interface DropIndicator {
  nodeId: string;
  position: DropPosition;
}

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

  const buildSiblingMovesForParent = useCallback((parentNodeId: string | null): [number, number][] => {
    const siblings = folderNodes
      .filter((n) => (n.parent_id ?? null) === parentNodeId)
      .sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0));
    return siblings
      .map((n) => {
        const id = parseFolderId(n.id);
        if (id == null) return null;
        return [id, n.sort_order ?? 0] as [number, number];
      })
      .filter((m): m is [number, number] => m != null);
  }, [folderNodes]);

  const handleRenameCommit = useCallback(async (id: string, newName: string) => {
    const folderId = parseFolderId(id);
    if (folderId != null) {
      try {
        const currentNode = folderNodes.find((n) => n.id === id);
        const oldName = currentNode?.name ?? '';
        if (oldName === newName) return;
        await FolderController.updateFolder({ folderId, name: newName });
        registerUndoAction({
          label: 'Rename folder',
          undo: async () => {
            await FolderController.updateFolder({ folderId, name: oldName });
          },
          redo: async () => {
            await FolderController.updateFolder({ folderId, name: newName });
          },
        });
      } catch (e) { console.error('Rename failed:', e); }
    }
  }, [folderNodes]);
  const {
    renamingId, renameValue, startRename, setRenameValue,
    commitRename, renameInputRef, renameKeyHandler,
  } = useInlineRename(handleRenameCommit);
  const pendingRenameFolderId = useRef<number | null>(null);

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

  const contextMenu = useContextMenu();
  const [contextMenuNodeId, setContextMenuNodeId] = useState<string | null>(null);
  const dragDropTarget = useImageDragDropTarget();

  const [activeId, setActiveId] = useState<string | null>(null);
  const [dropIndicator, setDropIndicator] = useState<DropIndicator | null>(null);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
  );

  useEffect(() => {
    if (pendingRenameFolderId.current == null) return;
    const pendingId = `folder:${pendingRenameFolderId.current}`;
    const found = folderNodes.find((n) => n.id === pendingId);
    if (found) {
      startRename(pendingId, found.name);
      pendingRenameFolderId.current = null;
      if (found.parent_id) {
        setCollapsedNodes((prev) => { const next = new Set(prev); next.delete(found.parent_id!); return next; });
      }
    }
  }, [folderNodes]);

  const handleCreate = useCallback(async () => {
    try {
      let folder = await FolderController.createFolder({ name: 'New Folder' });
      pendingRenameFolderId.current = folder.folder_id;
      startRename(`folder:${folder.folder_id}`, folder.name);
      registerUndoAction({
        label: 'Create folder',
        undo: async () => {
          await FolderController.deleteFolder(folder.folder_id);
        },
        redo: async () => {
          folder = await FolderController.createFolder({ name: folder.name });
        },
      });
    } catch (e) {
      console.error('Failed to create folder:', e);
    }
  }, []);

  const handleDelete = useCallback(async (nodeId: string) => {
    const folderId = parseFolderId(nodeId);
    if (folderId == null) return;
    try {
      const node = nodeMap.get(nodeId);
      const hasChildren = (node?.children.length ?? 0) > 0;
      const snapshot = !hasChildren && node ? {
        name: node.name,
        parentId: node.parent_id ? parseFolderId(node.parent_id) : null,
        icon: node.icon ?? null,
        color: node.color ?? null,
        files: await FolderController.getFolderFiles(folderId),
      } : null;
      await FolderController.deleteFolder(folderId);
      if (snapshot) {
        let recreatedId: number | null = null;
        registerUndoAction({
          label: `Delete folder "${snapshot.name}"`,
          undo: async () => {
            const recreated = await FolderController.createFolder({
              name: snapshot.name,
              parentId: snapshot.parentId,
              icon: snapshot.icon,
              color: snapshot.color,
            });
            recreatedId = recreated.folder_id;
            if (snapshot.files.length > 0) {
              await FolderController.addFilesToFolderBatch(recreated.folder_id, snapshot.files);
            }
          },
          redo: async () => {
            const id = recreatedId ?? folderId;
            await FolderController.deleteFolder(id);
          },
        });
      } else {
        notifyWarning('Undo for deleting folders with subfolders is not supported yet.', 'Limited Undo');
      }
      if (activeFolder?.folder_id === folderId) setActiveFolder(null);
    } catch (e) { console.error('Delete failed:', e); }
  }, [activeFolder, setActiveFolder, nodeMap]);

  const handleBatchDelete = useCallback(async (ids: Set<string>) => {
    const folderIds = [...ids].map(parseFolderId).filter((id): id is number => id != null);
    if (folderIds.length === 0) return;
    try {
      await Promise.all(folderIds.map((id) => FolderController.deleteFolder(id)));
      notifyWarning('Batch folder delete cannot be undone yet.', 'Limited Undo');
      setSelectedIds(new Set());
      if (activeFolder && folderIds.includes(activeFolder.folder_id)) setActiveFolder(null);
    } catch (e) { console.error('Batch delete failed:', e); }
  }, [activeFolder, setActiveFolder]);

  const handleSortFolders = useCallback(async (parentId: string | null, direction: 'asc' | 'desc') => {
    const siblings = folderNodes
      .filter((n) => n.parent_id === parentId)
      .sort((a, b) => direction === 'asc' ? a.name.localeCompare(b.name) : b.name.localeCompare(a.name));
    const previousMoves: [number, number][] = siblings
      .map((n) => {
        const fid = parseFolderId(n.id);
        if (fid == null) return null;
        return [fid, n.sort_order ?? 0] as [number, number];
      })
      .filter((m): m is [number, number] => m != null);
    const moves: [number, number][] = [];
    siblings.forEach((n, i) => {
      const fid = parseFolderId(n.id);
      if (fid != null) moves.push([fid, (i + 1) * 1000]);
    });
    try {
      await FolderController.reorderFolders(moves);
      registerUndoAction({
        label: 'Sort folders',
        undo: async () => {
          await FolderController.reorderFolders(previousMoves);
        },
        redo: async () => {
          await FolderController.reorderFolders(moves);
        },
      });
    } catch (err) { console.error('Sort failed:', err); }
  }, [folderNodes]);

  const handleSortAllFolders = useCallback(async (direction: 'asc' | 'desc') => {
    const previousMoves: [number, number][] = folderNodes
      .map((n) => {
        const fid = parseFolderId(n.id);
        if (fid == null) return null;
        return [fid, n.sort_order ?? 0] as [number, number];
      })
      .filter((m): m is [number, number] => m != null);
    const parentGroups = new Map<string | null, typeof folderNodes>();
    for (const n of folderNodes) {
      const key = n.parent_id ?? null;
      if (!parentGroups.has(key)) parentGroups.set(key, []);
      parentGroups.get(key)!.push(n);
    }
    const allMoves: [number, number][] = [];
    for (const [, siblings] of parentGroups) {
      siblings.sort((a, b) => direction === 'asc' ? a.name.localeCompare(b.name) : b.name.localeCompare(a.name));
      siblings.forEach((n, i) => {
        const fid = parseFolderId(n.id);
        if (fid != null) allMoves.push([fid, (i + 1) * 1000]);
      });
    }
    try {
      await FolderController.reorderFolders(allMoves);
      registerUndoAction({
        label: 'Sort all folders',
        undo: async () => {
          await FolderController.reorderFolders(previousMoves);
        },
        redo: async () => {
          await FolderController.reorderFolders(allMoves);
        },
      });
    } catch (err) { console.error('Sort all failed:', err); }
  }, [folderNodes]);

  const expandFolder = useCallback((nodeId: string) => {
    setCollapsedNodes((prev) => { const next = new Set(prev); next.delete(nodeId); return next; });
  }, []);

  const expandSameLevel = useCallback((node: TreeNode) => {
    const siblings = folderNodes.filter((n) => n.parent_id === node.parent_id);
    setCollapsedNodes((prev) => {
      const next = new Set(prev);
      for (const s of siblings) next.delete(s.id);
      return next;
    });
  }, [folderNodes]);

  const expandAll = useCallback(() => {
    setCollapsedNodes(new Set());
  }, []);

  const collapseAll = useCallback(() => {
    const allIds = new Set(folderNodes.filter((n) => {
      return folderNodes.some((c) => c.parent_id === n.id);
    }).map((n) => n.id));
    setCollapsedNodes(allIds);
  }, [folderNodes]);

  const createSiblingFolder = useCallback(async (node: TreeNode) => {
    const parentId = node.parent_id ? parseFolderId(node.parent_id) : null;
    let folder = await FolderController.createFolder({ name: 'New Folder', parentId });
    pendingRenameFolderId.current = folder.folder_id;
    startRename(`folder:${folder.folder_id}`, folder.name);
    registerUndoAction({
      label: 'Create folder',
      undo: async () => {
        await FolderController.deleteFolder(folder.folder_id);
      },
      redo: async () => {
        folder = await FolderController.createFolder({ name: folder.name, parentId });
      },
    });
  }, [startRename]);

  const createSubfolderForNode = useCallback(async (node: TreeNode, folderId: number) => {
    expandFolder(node.id);
    let sub = await FolderController.createFolder({ name: 'New Folder', parentId: folderId });
    pendingRenameFolderId.current = sub.folder_id;
    startRename(`folder:${sub.folder_id}`, sub.name);
    registerUndoAction({
      label: 'Create subfolder',
      undo: async () => {
        await FolderController.deleteFolder(sub.folder_id);
      },
      redo: async () => {
        sub = await FolderController.createFolder({ name: sub.name, parentId: folderId });
      },
    });
  }, [expandFolder, startRename]);

  const applyIconToFolders = useCallback(async (ids: number[], icon: string | null) => {
    const previous = ids.map((id) => {
      const targetNode = folderNodes.find((n) => parseFolderId(n.id) === id);
      return { id, icon: targetNode?.icon ?? null };
    });
    await Promise.all(ids.map((id) => FolderController.updateFolder({ folderId: id, icon })));
    registerUndoAction({
      label: ids.length > 1 ? 'Change folder icons' : 'Change folder icon',
      undo: async () => {
        await Promise.all(previous.map((entry) => FolderController.updateFolder({ folderId: entry.id, icon: entry.icon })));
      },
      redo: async () => {
        await Promise.all(ids.map((id) => FolderController.updateFolder({ folderId: id, icon })));
      },
    });
  }, [folderNodes]);

  const applyColorToFolders = useCallback(async (ids: number[], color: string | null) => {
    const previous = ids.map((id) => {
      const targetNode = folderNodes.find((n) => parseFolderId(n.id) === id);
      return { id, color: targetNode?.color ?? null };
    });
    await Promise.all(ids.map((id) => FolderController.updateFolder({ folderId: id, color })));
    registerUndoAction({
      label: ids.length > 1 ? 'Change folder colors' : 'Change folder color',
      undo: async () => {
        await Promise.all(previous.map((entry) => FolderController.updateFolder({ folderId: entry.id, color: entry.color })));
      },
      redo: async () => {
        await Promise.all(ids.map((id) => FolderController.updateFolder({ folderId: id, color })));
      },
    });
  }, [folderNodes]);

  const openFolderAutoTagsEditor = useCallback((folderId: number, folderName: string, currentTags: string[]) => {
    const original = [...currentTags];
    let draft = [...currentTags];
    TagSelectService.open({
      mode: 'modal',
      title: `Auto-Tags · ${folderName}`,
      anchorEl: null,
      selectedTags: draft,
      onToggle: (tag, added) => {
        draft = added ? [...draft, tag] : draft.filter((entry) => entry !== tag);
      },
      onClose: () => {
        const next = Array.from(new Set(draft)).sort();
        const prev = Array.from(new Set(original)).sort();
        if (JSON.stringify(next) === JSON.stringify(prev)) return;
        void FolderController.updateFolder({ folderId, autoTags: next }).then(() => {
          registerUndoAction({
            label: 'Update folder auto-tags',
            undo: async () => {
              await FolderController.updateFolder({ folderId, autoTags: prev });
            },
            redo: async () => {
              await FolderController.updateFolder({ folderId, autoTags: next });
            },
          });
        }).catch((error) => {
          console.error('Failed to update folder auto-tags:', error);
        });
      },
    });
  }, []);

  const toggleSameLevelFolders = useCallback((node: TreeNode) => {
    const siblings = folderNodes.filter((entry) => entry.parent_id === node.parent_id);
    const anyCollapsed = siblings.some((entry) => collapsedNodes.has(entry.id));
    if (anyCollapsed) {
      expandSameLevel(node);
      return;
    }
    setCollapsedNodes((prev) => {
      const next = new Set(prev);
      for (const sibling of siblings) {
        if (folderNodes.some((child) => child.parent_id === sibling.id)) next.add(sibling.id);
      }
      return next;
    });
  }, [collapsedNodes, expandSameLevel, folderNodes]);

  const handleContextMenu = useCallback((e: React.MouseEvent, node: TreeNode) => {
    setContextMenuNodeId(node.id);
    const folderId = parseFolderId(node.id);
    if (folderId == null) return;
    const isMulti = selectedIds.size > 1 && selectedIds.has(node.id);

    if (isMulti) {
      const ids = [...selectedIds].map(parseFolderId).filter((id): id is number => id != null);
      const items = buildFolderMultiMenu({
        sortBy: {
          currentLevelAsc: () => handleSortFolders(node.parent_id, 'asc'),
          currentLevelDesc: () => handleSortFolders(node.parent_id, 'desc'),
          allLevelsAsc: () => handleSortAllFolders('asc'),
          allLevelsDesc: () => handleSortAllFolders('desc'),
        },
        iconAndColor: {
          onIconChange: (icon) => applyIconToFolders(ids, icon),
          onColorChange: (color) => applyColorToFolders(ids, color),
          iconLabel: 'Change Icon...',
        },
        deleteFolders: () => handleBatchDelete(selectedIds),
        deleteLabel: `Remove ${selectedIds.size} Folders`,
      });
      contextMenu.open(e, items);
      return;
    }

    const hasChildren = node.children.length > 0;
    const autoTags = getFolderAutoTags(node);
    const items: ContextMenuEntry[] = buildFolderSingleMenu({
      createFolder: () => createSiblingFolder(node),
      createSubfolder: () => createSubfolderForNode(node, folderId),
      renameFolder: () => startRename(node.id, node.name),
      setAutoTags: () => openFolderAutoTagsEditor(folderId, node.name, autoTags),
      sortBy: {
        currentLevelAsc: () => handleSortFolders(node.parent_id, 'asc'),
        currentLevelDesc: () => handleSortFolders(node.parent_id, 'desc'),
        allLevelsAsc: () => handleSortAllFolders('asc'),
        allLevelsDesc: () => handleSortAllFolders('desc'),
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
        onIconChange: (icon) => applyIconToFolders([folderId], icon),
        onColorChange: (color) => applyColorToFolders([folderId], color),
        iconLabel: 'Change Icon...',
      },
      deleteFolder: () => handleDelete(node.id),
      deleteLabel: 'Remove Folder',
      showDuplicate: true,
      showExport: true,
    });
    contextMenu.open(e, items);
  }, [applyColorToFolders, applyIconToFolders, collapsedNodes.size, collapseAll, contextMenu, createSiblingFolder, createSubfolderForNode, expandAll, handleBatchDelete, handleDelete, handleSortAllFolders, handleSortFolders, openFolderAutoTagsEditor, selectedIds, startRename, toggleExpand, toggleSameLevelFolders]);

  const handleFilesDropOnFolder = useCallback(async (folderId: number, hashes: string[]) => {
    try {
      await FolderController.addFilesToFolderBatch(folderId, hashes);
      registerUndoAction({
        label: `Add ${hashes.length} to folder`,
        undo: async () => {
          await FolderController.removeFilesFromFolderBatch(folderId, hashes);
        },
        redo: async () => {
          await FolderController.addFilesToFolderBatch(folderId, hashes);
        },
      });
    } catch (err) { console.error('Failed to add files to folder:', err); }
  }, []);

  useEffect(() => {
    return imageDrag.onDrop(async ({ hashes, folderId }) => {
      handleFilesDropOnFolder(folderId, hashes);
    });
  }, [handleFilesDropOnFolder]);

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
    if (!draggedNode || !overNode) { setDropIndicator(null); return; }

    const descendants = collectDescendantIds(draggedNode);
    if (descendants.has(overNode.id)) { setDropIndicator(null); return; }

    const overRect = over.rect;
    const cursorY = event.activatorEvent instanceof MouseEvent
      ? event.activatorEvent.clientY + (event.delta?.y ?? 0)
      : overRect.top + overRect.height / 2;
    const relativeY = cursorY - overRect.top;
    const ratio = relativeY / overRect.height;

    let position: DropPosition;
    if (ratio < 0.25) {
      position = 'before';
    } else if (ratio > 0.75) {
      position = 'after';
    } else {
      position = 'inside';
    }

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

    const draggedFolderId = parseFolderId(draggedNode.id);
    if (draggedFolderId == null) return;
    const draggedOldParentNodeId = draggedNode.parent_id ?? null;
    const oldParentFolderId = draggedOldParentNodeId ? parseFolderId(draggedOldParentNodeId) : null;
    const oldSiblingMoves = buildSiblingMovesForParent(draggedOldParentNodeId);
    let redoParentFolderId: number | null = oldParentFolderId;
    let redoSiblingMoves: [number, number][] = [];

    try {
      if (indicator.position === 'inside') {
        // PBI-057: Atomic reparent — use moveFolder with empty sibling order.
        const targetFolderId = parseFolderId(targetNode.id);
        if (targetFolderId == null) return;
        redoParentFolderId = targetFolderId;
        redoSiblingMoves = [];
        await FolderController.moveFolder(draggedFolderId, targetFolderId, []);
        setCollapsedNodes((prev) => { const next = new Set(prev); next.delete(targetNode.id); return next; });
      } else {
        // PBI-057: Atomic reparent + reorder in single transaction.
        const targetParentId = targetNode.parent_id;
        const newParentFolderId = targetParentId ? parseFolderId(targetParentId) : null;

        const siblingNodes = folderNodes.filter(
          (n) => n.parent_id === targetParentId && n.id !== draggedNode.id
        );
        siblingNodes.sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0));

        const targetIdx = siblingNodes.findIndex((n) => n.id === targetNode.id);
        const insertIdx = indicator.position === 'before' ? targetIdx : targetIdx + 1;
        const reordered = [...siblingNodes];
        reordered.splice(insertIdx, 0, draggedNode);

        const moves: [number, number][] = reordered.map((n, i) => {
          const folderId = parseInt(n.id.replace('folder:', ''), 10);
          return [folderId, (i + 1) * 1000];
        });
        redoParentFolderId = newParentFolderId;
        redoSiblingMoves = moves;
        await FolderController.moveFolder(draggedFolderId, newParentFolderId, moves);
      }

      registerUndoAction({
        label: 'Move folder',
        undo: async () => {
          await FolderController.moveFolder(draggedFolderId, oldParentFolderId, oldSiblingMoves);
        },
        redo: async () => {
          await FolderController.moveFolder(draggedFolderId, redoParentFolderId, redoSiblingMoves);
        },
      });
    } catch (err) {
      console.error('Folder DnD failed:', err);
    }
  }, [dropIndicator, nodeMap, folderNodes, buildSiblingMovesForParent]);

  const handleDragCancel = useCallback(() => {
    setActiveId(null);
    setDropIndicator(null);
  }, []);

  const sortableIds = flat.map((n) => n.id);
  const activeDragNode = activeId ? nodeMap.get(activeId) : null;

  return (
    <>
      <SidebarSection title="Folders" onAdd={handleCreate}>
        <DndContext
          sensors={sensors}
          onDragStart={handleDragStart}
          onDragMove={handleDragMove}
          onDragEnd={handleDragEnd}
          onDragCancel={handleDragCancel}
        >
          <SortableContext items={sortableIds} strategy={verticalListSortingStrategy}>
            {flat.map((node) => {
              const folderId = parseFolderId(node.id);
              const isActive = folderId != null && activeFolder?.folder_id === folderId;
              const isRenaming = renamingId === node.id;
              const hasChildren = node.children.length > 0;
              const isExpanded = !collapsedNodes.has(node.id);
              const autoTags = getFolderAutoTags(node);

              return (
                <SortableFolderRow key={node.id} node={node} dropIndicator={dropIndicator}>
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
                    onNativeDrop={handleFilesDropOnFolder}
                    dataFolderDropId={folderId}
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
