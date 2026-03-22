import { api } from '#desktop/api';
import { registerUndoAction } from '../shared/controllers/undoRedoController';
import { useDomainStore } from '../state/domainStore';
import { useNavigationStore } from '../state/navigationStore';
import type { SmartFolder, SmartFolderIpcInput, SmartFolderPredicate } from '../shared/types/api';

function eagerSidebarRefresh(): void {
  useDomainStore.getState().requestRefresh();
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
        redo: async () => { await api.smartFolders.delete(id); eagerSidebarRefresh(); },
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

  async move(
    smartFolderId: number,
    newParentId: number | null,
    siblingOrder: [number, number][],
    undoParams?: { oldParentId: number | null; oldSiblingMoves: [number, number][] },
  ) {
    await api.smartFolders.move(smartFolderId, newParentId, siblingOrder);
    eagerSidebarRefresh();
    if (undoParams) {
      registerUndoAction({
        label: 'Move smart folder',
        undo: async () => { await api.smartFolders.move(smartFolderId, undoParams.oldParentId, undoParams.oldSiblingMoves); eagerSidebarRefresh(); },
        redo: async () => { await api.smartFolders.move(smartFolderId, newParentId, siblingOrder); eagerSidebarRefresh(); },
      });
    }
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
