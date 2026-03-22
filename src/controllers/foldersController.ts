import { api } from '#desktop/api';
import { queryApi } from '#desktop/api';
import { registerUndoAction } from '../shared/controllers/undoRedoController';
import { useTaskStore } from '../state/taskStore';
import { useGridMetadataStore } from '../state/gridMetadataStore';
import { useDomainStore } from '../state/domainStore';
import { useNavigationStore } from '../state/navigationStore';
import type { Folder, FolderMembership, SelectionQuerySpec } from '../shared/types/api';
import type { FolderReorderMove } from '../shared/types/api';

// ── Eager UI helpers ─────────────────────────────────────────────────────────

/** Eagerly remove hashes from the grid if user is viewing the affected folder. */
function eagerGridRemove(folderId: number, hashes: string[]): void {
  if (useNavigationStore.getState().activeFolderId === folderId && hashes.length > 0) {
    useGridMetadataStore.getState().queueRemovals(hashes);
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
    useDomainStore.getState().insertFolderNode(folder.folder_id, params.name, params.parentId ?? null, params.icon, params.color);
    registerUndoAction({
      label: 'Create folder',
      backward: async () => { await api.folders.delete(folder.folder_id); useDomainStore.getState().removeFolderNode(folder.folder_id); },
      forward: async () => {
        const re = await api.folders.create({ name: folder.name, parent_id: params.parentId ?? null, icon: params.icon, color: params.color });
        useDomainStore.getState().insertFolderNode(re.folder_id, re.name, params.parentId ?? null, params.icon, params.color);
      },
    });
    return folder;
  },

  async rename(folderId: number, newName: string, oldName: string) {
    if (oldName === newName) return;
    await api.folders.update({ folder_id: folderId, name: newName });
    useDomainStore.getState().patchFolderNode(folderId, { name: newName });
    registerUndoAction({
      label: 'Rename folder',
      backward: async () => { await api.folders.update({ folder_id: folderId, name: oldName }); useDomainStore.getState().patchFolderNode(folderId, { name: oldName }); },
      forward: async () => { await api.folders.update({ folder_id: folderId, name: newName }); useDomainStore.getState().patchFolderNode(folderId, { name: newName }); },
    });
  },

  update(params: { folder_id: number; name?: string; icon?: string; color?: string; auto_tags?: string[] }) {
    return api.folders.update(params);
  },

  async delete(folderId: number, snapshot?: { name: string; parentId: number | null; icon: string | null; color: string | null; files: string[] } | null) {
    await api.folders.delete(folderId);
    useDomainStore.getState().removeFolderNode(folderId);
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
          useDomainStore.getState().insertFolderNode(recreated.folder_id, snapshot.name, snapshot.parentId, snapshot.icon, snapshot.color);
        },
        forward: async () => { await api.folders.delete(recreatedId ?? folderId); useDomainStore.getState().removeFolderNode(recreatedId ?? folderId); },
      });
    }
  },

  async deleteBatch(folderIds: number[], snapshots: Array<{ name: string; parentId: number | null }>) {
    await Promise.all(folderIds.map((id) => api.folders.delete(id)));
    for (const id of folderIds) useDomainStore.getState().removeFolderNode(id);
    const nav = useNavigationStore.getState();
    if (nav.activeFolderId != null && folderIds.includes(nav.activeFolderId)) {
      nav.navigateTo('images');
    }
    registerUndoAction({
      label: `Delete ${folderIds.length} folder${folderIds.length === 1 ? '' : 's'}`,
      backward: async () => {
        for (const snap of snapshots) {
          const re = await api.folders.create({ name: snap.name, parent_id: snap.parentId });
          useDomainStore.getState().insertFolderNode(re.folder_id, snap.name, snap.parentId);
        }
      },
      forward: async () => { /* best-effort */ },
    });
  },

  // ── Membership ─────────────────────────────────────────────────────────────

  async addFiles(folderId: number, hashes: string[], selection?: SelectionQuerySpec) {
    const count = hashes.length;
    if (count > 0) useDomainStore.getState().adjustFolderCount(folderId, count);
    await api.folders.addFiles(folderId, hashes, selection);
    if (useNavigationStore.getState().activeFolderId === folderId && hashes.length > 0) {
      Promise.all(hashes.map((h) => queryApi.file.get(h))).then((entities) => {
        const valid = entities.filter((e): e is NonNullable<typeof e> => e != null);
        if (valid.length > 0) useGridMetadataStore.getState().queueInsertions(valid);
      });
    }
    if (hashes.length > 0 && !selection) {
      registerUndoAction({
        label: `Add ${hashes.length} to folder`,
        backward: async () => { await api.folders.removeFiles(folderId, hashes); useDomainStore.getState().adjustFolderCount(folderId, -count); eagerGridRemove(folderId, hashes); },
        forward: async () => {
          await api.folders.addFiles(folderId, hashes);
          useDomainStore.getState().adjustFolderCount(folderId, count);
          if (useNavigationStore.getState().activeFolderId === folderId) {
            Promise.all(hashes.map((h) => queryApi.file.get(h))).then((entities) => {
              const valid = entities.filter((e): e is NonNullable<typeof e> => e != null);
              if (valid.length > 0) useGridMetadataStore.getState().queueInsertions(valid);
            });
          }
        },
      });
    }
  },

  async removeFiles(folderId: number, hashes: string[], selection?: SelectionQuerySpec) {
    const count = hashes.length;
    if (count > 0) useDomainStore.getState().adjustFolderCount(folderId, -count);
    eagerGridRemove(folderId, hashes);
    await api.folders.removeFiles(folderId, hashes, selection);
    if (hashes.length > 0 && !selection) {
      registerUndoAction({
        label: `Remove ${hashes.length} item${hashes.length === 1 ? '' : 's'} from folder`,
        backward: async () => {
          await api.folders.addFiles(folderId, hashes);
          useDomainStore.getState().adjustFolderCount(folderId, count);
          if (useNavigationStore.getState().activeFolderId === folderId) {
            Promise.all(hashes.map((h) => queryApi.file.get(h))).then((entities) => {
              const valid = entities.filter((e): e is NonNullable<typeof e> => e != null);
              if (valid.length > 0) useGridMetadataStore.getState().queueInsertions(valid);
            });
          }
        },
        forward: async () => {
          await api.folders.removeFiles(folderId, hashes);
          useDomainStore.getState().adjustFolderCount(folderId, -count);
          eagerGridRemove(folderId, hashes);
        },
      });
    }
  },

  // ── Item ordering ──────────────────────────────────────────────────────────

  async sortItems(folderId: number, sortBy: string, direction: string, hashes?: string[]) {
    await api.folders.sortItems(folderId, sortBy, direction, hashes);
    useGridMetadataStore.getState().requestScopedReplace(`folder:${folderId}`);
  },

  async reverseItems(folderId: number, hashes?: string[]) {
    await api.folders.reverseItems(folderId, hashes);
    useGridMetadataStore.getState().requestScopedReplace(`folder:${folderId}`);
  },

  async reorderItems(folderId: number, moves: FolderReorderMove[]) {
    // Caller (useGridReorder) already did local reorder via dispatch SET_IMAGES.
    // Controller just persists to backend — no grid update needed.
    await api.folders.reorderItems(folderId, moves);
  },

  // ── Folder ordering ────────────────────────────────────────────────────────

  async reorder(moves: [number, number][], previousMoves?: [number, number][]) {
    await api.folders.reorder(moves);
    useDomainStore.getState().reorderFolderNodes(moves);
    if (previousMoves) {
      registerUndoAction({
        label: 'Reorder folders',
        backward: async () => { await api.folders.reorder(previousMoves); useDomainStore.getState().reorderFolderNodes(previousMoves); },
        forward: async () => { await api.folders.reorder(moves); useDomainStore.getState().reorderFolderNodes(moves); },
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
    useDomainStore.getState().moveFolderNode(folderId, newParentId);
    useDomainStore.getState().reorderFolderNodes(siblingOrder);
    if (undoParams) {
      registerUndoAction({
        label: 'Move folder',
        backward: async () => {
          await api.folders.moveFolder(folderId, undoParams.oldParentId, undoParams.oldSiblingMoves);
          useDomainStore.getState().moveFolderNode(folderId, undoParams.oldParentId);
          useDomainStore.getState().reorderFolderNodes(undoParams.oldSiblingMoves);
        },
        forward: async () => {
          await api.folders.moveFolder(folderId, newParentId, siblingOrder);
          useDomainStore.getState().moveFolderNode(folderId, newParentId);
          useDomainStore.getState().reorderFolderNodes(siblingOrder);
        },
      });
    }
  },

  // ── Icon / color / auto-tags ───────────────────────────────────────────────

  async applyIcon(ids: number[], icon: string | null, previousValues: Array<{ id: number; icon: string | null }>) {
    const value = icon === null ? '' : (icon ?? undefined);
    await Promise.all(ids.map((id) => api.folders.update({ folder_id: id, icon: value })));
    for (const id of ids) useDomainStore.getState().patchFolderNode(id, { icon });
    registerUndoAction({
      label: ids.length > 1 ? 'Change folder icons' : 'Change folder icon',
      backward: async () => {
        await Promise.all(previousValues.map((e) => api.folders.update({ folder_id: e.id, icon: e.icon === null ? '' : (e.icon ?? undefined) })));
        for (const e of previousValues) useDomainStore.getState().patchFolderNode(e.id, { icon: e.icon });
      },
      forward: async () => {
        await Promise.all(ids.map((id) => api.folders.update({ folder_id: id, icon: value })));
        for (const id of ids) useDomainStore.getState().patchFolderNode(id, { icon });
      },
    });
  },

  async applyColor(ids: number[], color: string | null, previousValues: Array<{ id: number; color: string | null }>) {
    const value = color === null ? '' : (color ?? undefined);
    await Promise.all(ids.map((id) => api.folders.update({ folder_id: id, color: value })));
    for (const id of ids) useDomainStore.getState().patchFolderNode(id, { color });
    registerUndoAction({
      label: ids.length > 1 ? 'Change folder colors' : 'Change folder color',
      backward: async () => {
        await Promise.all(previousValues.map((e) => api.folders.update({ folder_id: e.id, color: e.color === null ? '' : (e.color ?? undefined) })));
        for (const e of previousValues) useDomainStore.getState().patchFolderNode(e.id, { color: e.color });
      },
      forward: async () => {
        await Promise.all(ids.map((id) => api.folders.update({ folder_id: id, color: value })));
        for (const id of ids) useDomainStore.getState().patchFolderNode(id, { color });
      },
    });
  },

  async updateAutoTags(folderId: number, next: string[], prev: string[]) {
    await api.folders.update({ folder_id: folderId, auto_tags: next });
    registerUndoAction({
      label: 'Update folder auto-tags',
      backward: async () => { await api.folders.update({ folder_id: folderId, auto_tags: prev }); },
      forward: async () => { await api.folders.update({ folder_id: folderId, auto_tags: next }); },
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
      // Watch config doesn't change sidebar tree structure or counts
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
    // Watch config doesn't change sidebar tree structure or counts
  },
};
