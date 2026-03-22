import { api } from '#desktop/api';
import { registerUndoAction } from '../shared/controllers/undoRedoController';
import { useTaskStore } from '../state/taskStore';
import { useGridMetadataStore } from '../state/gridMetadataStore';
import { useDomainStore } from '../state/domainStore';
import { useNavigationStore } from '../state/navigationStore';
import type { Folder, FolderMembership, SelectionQuerySpec } from '../shared/types/api';
import type { FolderReorderMove } from '../shared/types/api';

// ── Eager UI helpers ─────────────────────────────────────────────────────────

/** Eagerly refresh sidebar tree + counts after any folder mutation. */
function eagerSidebarRefresh(): void {
  useDomainStore.getState().requestRefresh();
}

/** Eagerly remove hashes from the grid if user is viewing the affected folder. */
function eagerGridRemove(folderId: number, hashes: string[]): void {
  if (useNavigationStore.getState().activeFolderId === folderId && hashes.length > 0) {
    useGridMetadataStore.getState().queueRemovals(hashes);
  }
}

/** Eagerly signal the grid to re-fetch if user is viewing the affected folder. */
function eagerGridRefresh(folderId: number): void {
  if (useNavigationStore.getState().activeFolderId === folderId) {
    useGridMetadataStore.getState().queueClearAll();
  }
}

// ── Controller ───────────────────────────────────────────────────────────────

export const foldersController = {
  // ── Reads ──────────────────────────────────────────────────────────────────

  list(): Promise<Folder[]> {
    return api.folders.list();
  },

  getFileFolders(hash: string): Promise<FolderMembership[]> {
    return api.folders.getFileFolders(hash);
  },

  getEntityFolders(entityId: number): Promise<FolderMembership[]> {
    return api.folders.getEntityFolders(entityId);
  },

  getFiles(folderId: number) {
    return api.folders.getFiles(folderId);
  },

  getCoverHash(folderId: number) {
    return api.folders.getCoverHash(folderId);
  },

  // ── CRUD ───────────────────────────────────────────────────────────────────

  async create(params: { name: string; parentId?: number | null; icon?: string; color?: string }): Promise<Folder> {
    const folder = await api.folders.create({
      name: params.name,
      parent_id: params.parentId ?? null,
      icon: params.icon,
      color: params.color,
    });
    eagerSidebarRefresh();
    registerUndoAction({
      label: 'Create folder',
      backward: async () => { await api.folders.delete(folder.folder_id); eagerSidebarRefresh(); },
      forward: async () => { await api.folders.create({ name: folder.name, parent_id: params.parentId ?? null, icon: params.icon, color: params.color }); eagerSidebarRefresh(); },
    });
    return folder;
  },

  async rename(folderId: number, newName: string, oldName: string) {
    if (oldName === newName) return;
    await api.folders.update({ folder_id: folderId, name: newName });
    eagerSidebarRefresh();
    registerUndoAction({
      label: 'Rename folder',
      backward: async () => { await api.folders.update({ folder_id: folderId, name: oldName }); eagerSidebarRefresh(); },
      forward: async () => { await api.folders.update({ folder_id: folderId, name: newName }); eagerSidebarRefresh(); },
    });
  },

  update(params: { folder_id: number; name?: string; icon?: string; color?: string; auto_tags?: string[] }) {
    return api.folders.update(params);
  },

  async delete(folderId: number, snapshot?: { name: string; parentId: number | null; icon: string | null; color: string | null; files: string[] } | null) {
    await api.folders.delete(folderId);
    eagerSidebarRefresh();
    // If viewing the deleted folder, navigate away
    if (useNavigationStore.getState().activeFolderId === folderId) {
      useNavigationStore.getState().navigateTo('images');
    }
    if (snapshot) {
      let recreatedId: number | null = null;
      registerUndoAction({
        label: `Delete folder "${snapshot.name}"`,
        backward: async () => {
          const recreated = await api.folders.create({
            name: snapshot.name, parent_id: snapshot.parentId,
            icon: snapshot.icon ?? undefined, color: snapshot.color ?? undefined,
          });
          recreatedId = recreated.folder_id;
          if (snapshot.files.length > 0) {
            await api.folders.addFiles(recreated.folder_id, snapshot.files);
          }
          eagerSidebarRefresh();
        },
        forward: async () => { await api.folders.delete(recreatedId ?? folderId); eagerSidebarRefresh(); },
      });
    }
  },

  async deleteBatch(folderIds: number[], snapshots: Array<{ name: string; parentId: number | null }>) {
    await Promise.all(folderIds.map((id) => api.folders.delete(id)));
    eagerSidebarRefresh();
    const nav = useNavigationStore.getState();
    if (nav.activeFolderId != null && folderIds.includes(nav.activeFolderId)) {
      nav.navigateTo('images');
    }
    registerUndoAction({
      label: `Delete ${folderIds.length} folder${folderIds.length === 1 ? '' : 's'}`,
      backward: async () => {
        for (const snap of snapshots) {
          await api.folders.create({ name: snap.name, parent_id: snap.parentId });
        }
        eagerSidebarRefresh();
      },
      forward: async () => { /* best-effort */ },
    });
  },

  // ── Membership ─────────────────────────────────────────────────────────────

  async addFiles(folderId: number, hashes: string[], selection?: SelectionQuerySpec) {
    await api.folders.addFiles(folderId, hashes, selection);
    eagerSidebarRefresh();
    eagerGridRefresh(folderId);
    if (hashes.length > 0 && !selection) {
      registerUndoAction({
        label: `Add ${hashes.length} to folder`,
        backward: async () => { await api.folders.removeFiles(folderId, hashes); eagerSidebarRefresh(); eagerGridRemove(folderId, hashes); },
        forward: async () => { await api.folders.addFiles(folderId, hashes); eagerSidebarRefresh(); eagerGridRefresh(folderId); },
      });
    }
  },

  async removeFiles(folderId: number, hashes: string[], selection?: SelectionQuerySpec) {
    await api.folders.removeFiles(folderId, hashes, selection);
    eagerSidebarRefresh();
    eagerGridRemove(folderId, hashes);
    if (hashes.length > 0 && !selection) {
      registerUndoAction({
        label: `Remove ${hashes.length} file${hashes.length === 1 ? '' : 's'} from folder`,
        backward: async () => { await api.folders.addFiles(folderId, hashes); eagerSidebarRefresh(); eagerGridRefresh(folderId); },
        forward: async () => { await api.folders.removeFiles(folderId, hashes); eagerSidebarRefresh(); eagerGridRemove(folderId, hashes); },
      });
    }
  },

  // ── Item ordering ──────────────────────────────────────────────────────────

  async sortItems(folderId: number, sortBy: string, direction: string, hashes?: string[]) {
    await api.folders.sortItems(folderId, sortBy, direction, hashes);
    eagerGridRefresh(folderId);
  },

  async reverseItems(folderId: number, hashes?: string[]) {
    await api.folders.reverseItems(folderId, hashes);
    eagerGridRefresh(folderId);
  },

  async reorderItems(folderId: number, moves: FolderReorderMove[]) {
    await api.folders.reorderItems(folderId, moves);
    eagerGridRefresh(folderId);
  },

  // ── Folder ordering ────────────────────────────────────────────────────────

  async reorder(moves: [number, number][], previousMoves?: [number, number][]) {
    await api.folders.reorder(moves);
    eagerSidebarRefresh();
    if (previousMoves) {
      registerUndoAction({
        label: 'Reorder folders',
        backward: async () => { await api.folders.reorder(previousMoves); eagerSidebarRefresh(); },
        forward: async () => { await api.folders.reorder(moves); eagerSidebarRefresh(); },
      });
    }
  },

  async move(
    folderId: number,
    newParentId: number | null,
    siblingOrder: [number, number][],
    undoParams?: { oldParentId: number | null; oldSiblingMoves: [number, number][] },
  ) {
    await api.folders.moveFolder(folderId, newParentId, siblingOrder);
    eagerSidebarRefresh();
    if (undoParams) {
      registerUndoAction({
        label: 'Move folder',
        backward: async () => { await api.folders.moveFolder(folderId, undoParams.oldParentId, undoParams.oldSiblingMoves); eagerSidebarRefresh(); },
        forward: async () => { await api.folders.moveFolder(folderId, newParentId, siblingOrder); eagerSidebarRefresh(); },
      });
    }
  },

  // ── Icon / color / auto-tags ───────────────────────────────────────────────

  async applyIcon(ids: number[], icon: string | null, previousValues: Array<{ id: number; icon: string | null }>) {
    const value = icon === null ? '' : (icon ?? undefined);
    await Promise.all(ids.map((id) => api.folders.update({ folder_id: id, icon: value })));
    eagerSidebarRefresh();
    registerUndoAction({
      label: ids.length > 1 ? 'Change folder icons' : 'Change folder icon',
      backward: async () => {
        await Promise.all(previousValues.map((e) => api.folders.update({ folder_id: e.id, icon: e.icon === null ? '' : (e.icon ?? undefined) })));
        eagerSidebarRefresh();
      },
      forward: async () => {
        await Promise.all(ids.map((id) => api.folders.update({ folder_id: id, icon: value })));
        eagerSidebarRefresh();
      },
    });
  },

  async applyColor(ids: number[], color: string | null, previousValues: Array<{ id: number; color: string | null }>) {
    const value = color === null ? '' : (color ?? undefined);
    await Promise.all(ids.map((id) => api.folders.update({ folder_id: id, color: value })));
    eagerSidebarRefresh();
    registerUndoAction({
      label: ids.length > 1 ? 'Change folder colors' : 'Change folder color',
      backward: async () => {
        await Promise.all(previousValues.map((e) => api.folders.update({ folder_id: e.id, color: e.color === null ? '' : (e.color ?? undefined) })));
        eagerSidebarRefresh();
      },
      forward: async () => {
        await Promise.all(ids.map((id) => api.folders.update({ folder_id: id, color: value })));
        eagerSidebarRefresh();
      },
    });
  },

  async updateAutoTags(folderId: number, next: string[], prev: string[]) {
    await api.folders.update({ folder_id: folderId, auto_tags: next });
    eagerSidebarRefresh();
    registerUndoAction({
      label: 'Update folder auto-tags',
      backward: async () => { await api.folders.update({ folder_id: folderId, auto_tags: prev }); eagerSidebarRefresh(); },
      forward: async () => { await api.folders.update({ folder_id: folderId, auto_tags: next }); eagerSidebarRefresh(); },
    });
  },

  // ── Watch config ───────────────────────────────────────────────────────────

  async setWatchConfig(params: {
    folder_id: number;
    watch_path: string;
    watch_enabled?: boolean;
    watch_subfolders: boolean;
    watch_import_status_mode: 'inherit' | 'inbox' | 'active';
    import_existing_now: boolean;
  }) {
    if (params.import_existing_now) {
      useTaskStore.getState().startFamily('import', 'Adding files');
    }
    try {
      await api.folders.setWatchConfig(params);
      eagerSidebarRefresh();
      if (params.import_existing_now) {
        useTaskStore.getState().finishFamily('import');
      }
    } catch (err) {
      if (params.import_existing_now) {
        useTaskStore.getState().failFamily('import');
      }
      throw err;
    }
  },

  async clearWatchConfig(folderId: number) {
    await api.folders.clearWatchConfig(folderId);
    eagerSidebarRefresh();
  },
};
