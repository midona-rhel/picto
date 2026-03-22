import { api } from '#desktop/api';
import { registerUndoAction } from '../shared/controllers/undoRedoController';
import { useDomainStore } from '../state/domainStore';
import { useNavigationStore } from '../state/navigationStore';
import type { SmartFolder, SmartFolderIpcInput, SmartFolderPredicate } from '../shared/types/api';

function eagerSidebarRefresh(): void {
  useDomainStore.getState().requestRefresh();
}

/** Compute current sibling ordering for a parent (used for undo snapshots). */
function computeSiblingMoves(
  folders: Array<{ id: string; parent_id: number | null; display_order: number | null }>,
  parentId: number | null,
): [number, number][] {
  return folders
    .filter((f) => f.parent_id === parentId)
    .sort((a, b) => {
      const aOrd = a.display_order ?? Number.MAX_SAFE_INTEGER;
      const bOrd = b.display_order ?? Number.MAX_SAFE_INTEGER;
      if (aOrd !== bOrd) return aOrd - bOrd;
      return a.id.localeCompare(b.id, undefined, { numeric: true });
    })
    .map((f, i) => [parseInt(f.id, 10), (i + 1) * 1000]);
}

export const smartFoldersController = {
  // ── Reads ──────────────────────────────────────────────────────────────────

  count(predicate: SmartFolderPredicate): Promise<number> {
    return api.smartFolders.count(predicate);
  },

  // ── CRUD ───────────────────────────────────────────────────────────────────

  async create(folder: SmartFolderIpcInput, label = 'Create smart folder'): Promise<SmartFolder> {
    let created = await api.smartFolders.create(folder);
    eagerSidebarRefresh();
    registerUndoAction({
      label,
      undo: async () => { if (created?.id) { await api.smartFolders.delete(created.id); eagerSidebarRefresh(); } },
      redo: async () => { created = await api.smartFolders.create(folder); eagerSidebarRefresh(); },
    });
    return created;
  },

  async update(id: string, folder: SmartFolderIpcInput, beforeData?: SmartFolderIpcInput): Promise<SmartFolder> {
    const updated = await api.smartFolders.update(id, folder);
    eagerSidebarRefresh();
    if (beforeData) {
      registerUndoAction({
        label: 'Update smart folder',
        undo: async () => { await api.smartFolders.update(id, beforeData); eagerSidebarRefresh(); },
        redo: async () => { await api.smartFolders.update(id, folder); eagerSidebarRefresh(); },
      });
    }
    return updated;
  },

  async delete(id: string, snapshotData?: SmartFolderIpcInput) {
    await api.smartFolders.delete(id);
    eagerSidebarRefresh();
    if (useNavigationStore.getState().activeSmartFolderId === id) {
      useNavigationStore.getState().navigateTo('images');
    }
    if (snapshotData) {
      registerUndoAction({
        label: 'Delete smart folder',
        undo: async () => { await api.smartFolders.create(snapshotData); eagerSidebarRefresh(); },
        redo: async () => { /* ID differs after re-creation — cannot reliably re-delete */ },
      });
    }
  },

  async deleteBatch(ids: string[], snapshots?: SmartFolderIpcInput[]) {
    for (const id of ids) {
      await api.smartFolders.delete(id);
    }
    eagerSidebarRefresh();
    const activeId = useNavigationStore.getState().activeSmartFolderId;
    if (activeId && ids.includes(activeId)) {
      useNavigationStore.getState().navigateTo('images');
    }
    if (snapshots && snapshots.length > 0) {
      registerUndoAction({
        label: `Delete ${ids.length} smart folder${ids.length === 1 ? '' : 's'}`,
        undo: async () => {
          for (const snap of snapshots) { await api.smartFolders.create(snap); }
          eagerSidebarRefresh();
        },
        redo: async () => { /* best-effort: IDs will differ after re-creation */ },
      });
    }
  },

  // ── Tree ordering ──────────────────────────────────────────────────────────

  /**
   * Move a smart folder to a new position in the tree.
   * The controller computes sibling reorder moves and undo snapshots internally.
   *
   * @param draggedId  The folder being moved
   * @param targetId   The folder being dropped on (or next to)
   * @param position   'before' | 'after' | 'inside'
   * @param folders    The full flat folder list (id, parent_id, display_order) for computing siblings
   */
  async moveToPosition(
    draggedId: number,
    targetId: number,
    position: 'before' | 'after' | 'inside',
    folders: Array<{ id: string; parent_id: number | null; display_order: number | null }>,
  ) {
    const draggedIdStr = String(draggedId);
    const targetIdStr = String(targetId);

    // Snapshot old sibling order for undo
    const draggedFolder = folders.find((f) => f.id === draggedIdStr);
    const oldParentId = draggedFolder?.parent_id ?? null;
    const oldSiblingMoves = computeSiblingMoves(folders, oldParentId);

    let newParentId: number | null;
    let newSiblingMoves: [number, number][];

    if (position === 'inside') {
      newParentId = targetId;
      const siblings = folders
        .filter((f) => f.parent_id === targetId && f.id !== draggedIdStr)
        .sort((a, b) => (a.display_order ?? 0) - (b.display_order ?? 0));
      const reordered = [...siblings, folders.find((f) => f.id === draggedIdStr)!];
      newSiblingMoves = reordered.map((f, i) => [parseInt(f.id, 10), (i + 1) * 1000]);
    } else {
      const targetFolder = folders.find((f) => f.id === targetIdStr);
      newParentId = targetFolder?.parent_id ?? null;
      const siblings = folders
        .filter((f) => f.parent_id === newParentId && f.id !== draggedIdStr)
        .sort((a, b) => (a.display_order ?? 0) - (b.display_order ?? 0));
      const targetIdx = siblings.findIndex((f) => f.id === targetIdStr);
      const insertIdx = position === 'before' ? targetIdx : targetIdx + 1;
      const reordered = [...siblings];
      reordered.splice(insertIdx, 0, folders.find((f) => f.id === draggedIdStr)!);
      newSiblingMoves = reordered.map((f, i) => [parseInt(f.id, 10), (i + 1) * 1000]);
    }

    await api.smartFolders.move(draggedId, newParentId, newSiblingMoves);
    eagerSidebarRefresh();

    registerUndoAction({
      label: 'Move smart folder',
      undo: async () => { await api.smartFolders.move(draggedId, oldParentId, oldSiblingMoves); eagerSidebarRefresh(); },
      redo: async () => { await api.smartFolders.move(draggedId, newParentId, newSiblingMoves); eagerSidebarRefresh(); },
    });
  },

  async reorder(parentId: number | null, moves: [number, number][], previousMoves?: [number, number][]) {
    await api.smartFolders.reorder(parentId, moves);
    eagerSidebarRefresh();
    if (previousMoves) {
      registerUndoAction({
        label: 'Reorder smart folders',
        undo: async () => { await api.smartFolders.reorder(parentId, previousMoves); eagerSidebarRefresh(); },
        redo: async () => { await api.smartFolders.reorder(parentId, moves); eagerSidebarRefresh(); },
      });
    }
  },
};
