import { useCallback, useState } from 'react';
import {
  PointerSensor,
  useSensor,
  useSensors,
  type DragStartEvent,
  type DragEndEvent,
  type DragMoveEvent,
} from '@dnd-kit/core';
import { FolderController } from '../../../controllers/folderController';
import { registerUndoAction } from '../../../shared/controllers/undoRedoController';
import type { SidebarNodeDto } from '../../../shared/types/sidebar';
import {
  type TreeNode,
  type DropIndicator,
  type DropPosition,
  parseFolderId,
  collectDescendantIds,
} from '../lib/folderTreeData';

interface UseFolderTreeDndOptions {
  nodeMap: Map<string, TreeNode>;
  folderNodes: SidebarNodeDto[];
  setCollapsedNodes: React.Dispatch<React.SetStateAction<Set<string>>>;
}

export function useFolderTreeDnd({ nodeMap, folderNodes, setCollapsedNodes }: UseFolderTreeDndOptions) {
  const [activeId, setActiveId] = useState<string | null>(null);
  const [dropIndicator, setDropIndicator] = useState<DropIndicator | null>(null);
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
  );

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
  }, [dropIndicator, nodeMap, folderNodes, buildSiblingMovesForParent, setCollapsedNodes]);

  const handleDragCancel = useCallback(() => {
    setActiveId(null);
    setDropIndicator(null);
  }, []);

  return {
    sensors,
    activeId,
    dropIndicator,
    handleDragStart,
    handleDragMove,
    handleDragEnd,
    handleDragCancel,
  };
}
