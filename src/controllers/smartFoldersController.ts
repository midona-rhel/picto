import { api } from '#desktop/api';
import { registerUndoAction } from '../shared/controllers/undoRedoController';
import { useDomainStore } from '../state/domainStore';
import { useNavigationStore } from '../state/navigationStore';
import type { SmartFolder, SmartFolderIpcInput, SmartFolderPredicate } from '../shared/types/api';

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
    const sfId = parseInt(created.id ?? '0', 10);
    useDomainStore.getState().insertSmartFolder(sfId, folder.name, folder.parent_id ?? null, folder.icon ?? null, folder.color ?? null);
    registerUndoAction({
      label,
      backward: async () => { if (created?.id) { await api.smartFolders.delete(created.id); useDomainStore.getState().removeSmartFolder(parseInt(created.id, 10)); } },
      forward: async () => {
        created = await api.smartFolders.create(folder);
        useDomainStore.getState().insertSmartFolder(parseInt(created.id ?? '0', 10), folder.name, folder.parent_id ?? null, folder.icon ?? null, folder.color ?? null);
      },
    });
    return created;
  },

  async update(id: string, folder: SmartFolderIpcInput, beforeData?: SmartFolderIpcInput): Promise<SmartFolder> {
    const updated = await api.smartFolders.update(id, folder);
    const sfId = parseInt(id, 10);
    useDomainStore.getState().patchSmartFolder(sfId, { name: folder.name, icon: folder.icon, color: folder.color });
    if (beforeData) {
      registerUndoAction({
        label: 'Update smart folder',
        backward: async () => {
          await api.smartFolders.update(id, beforeData);
          useDomainStore.getState().patchSmartFolder(sfId, { name: beforeData.name, icon: beforeData.icon, color: beforeData.color });
        },
        forward: async () => {
          await api.smartFolders.update(id, folder);
          useDomainStore.getState().patchSmartFolder(sfId, { name: folder.name, icon: folder.icon, color: folder.color });
        },
      });
    }
    return updated;
  },

  async delete(id: string, snapshotData?: SmartFolderIpcInput) {
    const sfId = parseInt(id, 10);
    await api.smartFolders.delete(id);
    useDomainStore.getState().removeSmartFolder(sfId);
    if (useNavigationStore.getState().activeSmartFolderId === id) {
      useNavigationStore.getState().navigateTo('images');
    }
    if (snapshotData) {
      registerUndoAction({
        label: 'Delete smart folder',
        backward: async () => {
          const re = await api.smartFolders.create(snapshotData);
          useDomainStore.getState().insertSmartFolder(parseInt(re.id ?? '0', 10), snapshotData.name, snapshotData.parent_id ?? null, snapshotData.icon ?? null, snapshotData.color ?? null);
        },
        forward: async () => { /* ID differs after re-creation — cannot reliably re-delete */ },
      });
    }
  },

  async deleteBatch(ids: string[], snapshots?: SmartFolderIpcInput[]) {
    for (const id of ids) {
      await api.smartFolders.delete(id);
    }
    for (const id of ids) useDomainStore.getState().removeSmartFolder(parseInt(id, 10));
    const activeId = useNavigationStore.getState().activeSmartFolderId;
    if (activeId && ids.includes(activeId)) {
      useNavigationStore.getState().navigateTo('images');
    }
    if (snapshots && snapshots.length > 0) {
      registerUndoAction({
        label: `Delete ${ids.length} smart folder${ids.length === 1 ? '' : 's'}`,
        backward: async () => {
          for (const snap of snapshots) {
            const re = await api.smartFolders.create(snap);
            useDomainStore.getState().insertSmartFolder(parseInt(re.id ?? '0', 10), snap.name, snap.parent_id ?? null, snap.icon ?? null, snap.color ?? null);
          }
        },
        forward: async () => { /* best-effort: IDs will differ after re-creation */ },
      });
    }
  },

  // ── Tree ordering ──────────────────────────────────────────────────────────

  async moveToPosition(
    draggedId: number,
    targetId: number,
    position: 'before' | 'after' | 'inside',
    folders: Array<{ id: string; parent_id: number | null; display_order: number | null }>,
  ) {
    const draggedIdStr = String(draggedId);
    const targetIdStr = String(targetId);

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

    useDomainStore.getState().moveSmartFolderNode(draggedId, newParentId);
    useDomainStore.getState().reorderSmartFolderNodes(newSiblingMoves);
    await api.smartFolders.move(draggedId, newParentId, newSiblingMoves);

    registerUndoAction({
      label: 'Move smart folder',
      backward: async () => {
        await api.smartFolders.move(draggedId, oldParentId, oldSiblingMoves);
        useDomainStore.getState().moveSmartFolderNode(draggedId, oldParentId);
        useDomainStore.getState().reorderSmartFolderNodes(oldSiblingMoves);
      },
      forward: async () => {
        await api.smartFolders.move(draggedId, newParentId, newSiblingMoves);
        useDomainStore.getState().moveSmartFolderNode(draggedId, newParentId);
        useDomainStore.getState().reorderSmartFolderNodes(newSiblingMoves);
      },
    });
  },

  async reorder(parentId: number | null, moves: [number, number][], previousMoves?: [number, number][]) {
    useDomainStore.getState().reorderSmartFolderNodes(moves);
    await api.smartFolders.reorder(parentId, moves);
    if (previousMoves) {
      registerUndoAction({
        label: 'Reorder smart folders',
        backward: async () => { await api.smartFolders.reorder(parentId, previousMoves); useDomainStore.getState().reorderSmartFolderNodes(previousMoves); },
        forward: async () => { await api.smartFolders.reorder(parentId, moves); useDomainStore.getState().reorderSmartFolderNodes(moves); },
      });
    }
  },
};
