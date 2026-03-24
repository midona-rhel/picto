import { api } from '#desktop/api';
import { queryApi } from '#desktop/api';
import { registerUndoAction } from '../shared/controllers/undoRedoController';
import { useTaskStore } from '../state-legacy/taskStore';
import { useGridMetadataStore } from '../state-legacy/gridMetadataStore';
import { useNavigationStore } from '../state-legacy/navigationStore';
import { store } from '../state/store';
import { insertFolderNodeAtom, removeFolderNodeAtom, patchFolderNodeAtom, adjustFolderCountAtom, reorderFolderNodesAtom, moveFolderNodeAtom } from '../state/sidebar';
import type { SidebarNodeDto } from '../shared/types/sidebar';
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
    store.set(insertFolderNodeAtom, {
      id: `folder:${folder.folder_id}`,
      kind: 'folder',
      name: params.name,
      parent_id: params.parentId != null ? `folder:${params.parentId}` : null,
      icon: params.icon ?? null,
      color: params.color ?? null,
      count: 0,
      sort_order: null,
      freshness: 'fresh',
      selectable: true,
      expanded_by_default: false,
      meta: null,
    } as SidebarNodeDto);
    registerUndoAction({
      label: 'Create folder',
      backward: async () => { await api.folders.delete(folder.folder_id); store.set(removeFolderNodeAtom, folder.folder_id); },
      forward: async () => {
        const re = await api.folders.create({ name: folder.name, parent_id: params.parentId ?? null, icon: params.icon, color: params.color });
        store.set(insertFolderNodeAtom, {
          id: `folder:${re.folder_id}`,
          kind: 'folder',
          name: re.name,
          parent_id: params.parentId != null ? `folder:${params.parentId}` : null,
          icon: params.icon ?? null,
          color: params.color ?? null,
          count: 0,
          sort_order: null,
          freshness: 'fresh',
          selectable: true,
          expanded_by_default: false,
          meta: null,
        } as SidebarNodeDto);
      },
    });
    return folder;
  },

  async rename(folderId: number, newName: string, oldName: string) {
    if (oldName === newName) return;
    store.set(patchFolderNodeAtom, { folderId, patch: { name: newName } });
    await api.folders.update({ folder_id: folderId, name: newName });
    registerUndoAction({
      label: 'Rename folder',
      backward: async () => { await api.folders.update({ folder_id: folderId, name: oldName }); store.set(patchFolderNodeAtom, { folderId, patch: { name: oldName } }); },
      forward: async () => { await api.folders.update({ folder_id: folderId, name: newName }); store.set(patchFolderNodeAtom, { folderId, patch: { name: newName } }); },
    });
  },

  update(params: { folder_id: number; name?: string; icon?: string; color?: string; auto_tags?: string[] }) {
    return api.folders.update(params);
  },

  async delete(folderId: number, snapshot?: { name: string; parentId: number | null; icon: string | null; color: string | null; files: string[] } | null) {
    store.set(removeFolderNodeAtom, folderId);
    await api.folders.delete(folderId);
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
          store.set(insertFolderNodeAtom, {
            id: `folder:${recreated.folder_id}`,
            kind: 'folder',
            name: snapshot.name,
            parent_id: snapshot.parentId != null ? `folder:${snapshot.parentId}` : null,
            icon: snapshot.icon ?? null,
            color: snapshot.color ?? null,
            count: 0,
            sort_order: null,
            freshness: 'fresh',
            selectable: true,
            expanded_by_default: false,
            meta: null,
          } as SidebarNodeDto);
        },
        forward: async () => { await api.folders.delete(recreatedId ?? folderId); store.set(removeFolderNodeAtom, recreatedId ?? folderId); },
      });
    }
  },

  async deleteBatch(folderIds: number[], snapshots: Array<{ name: string; parentId: number | null }>) {
    for (const id of folderIds) store.set(removeFolderNodeAtom, id);
    await Promise.all(folderIds.map((id) => api.folders.delete(id)));
    const nav = useNavigationStore.getState();
    if (nav.activeFolderId != null && folderIds.includes(nav.activeFolderId)) {
      nav.navigateTo('images');
    }
    registerUndoAction({
      label: `Delete ${folderIds.length} folder${folderIds.length === 1 ? '' : 's'}`,
      backward: async () => {
        for (const snap of snapshots) {
          const re = await api.folders.create({ name: snap.name, parent_id: snap.parentId });
          store.set(insertFolderNodeAtom, {
            id: `folder:${re.folder_id}`,
            kind: 'folder',
            name: snap.name,
            parent_id: snap.parentId != null ? `folder:${snap.parentId}` : null,
            icon: null,
            color: null,
            count: 0,
            sort_order: null,
            freshness: 'fresh',
            selectable: true,
            expanded_by_default: false,
            meta: null,
          } as SidebarNodeDto);
        }
      },
      forward: async () => { /* best-effort */ },
    });
  },

  // ── Membership ─────────────────────────────────────────────────────────────

  async addFiles(folderId: number, hashes: string[], selection?: SelectionQuerySpec) {
    const count = hashes.length;
    if (count > 0) store.set(adjustFolderCountAtom, { folderId, delta: count });
    await api.folders.addFiles(folderId, hashes, selection);
    if (useNavigationStore.getState().activeFolderId === folderId && hashes.length > 0) {
      queryApi.file.getGridItems(hashes).then((entities) => {
        if (entities.length > 0) useGridMetadataStore.getState().queueInsertions(entities);
      });
    }
    if (hashes.length > 0 && !selection) {
      registerUndoAction({
        label: `Add ${hashes.length} to folder`,
        backward: async () => { await api.folders.removeFiles(folderId, hashes); store.set(adjustFolderCountAtom, { folderId, delta: -count }); eagerGridRemove(folderId, hashes); },
        forward: async () => {
          await api.folders.addFiles(folderId, hashes);
          store.set(adjustFolderCountAtom, { folderId, delta: count });
          if (useNavigationStore.getState().activeFolderId === folderId) {
            queryApi.file.getGridItems(hashes).then((entities) => {
              if (entities.length > 0) useGridMetadataStore.getState().queueInsertions(entities);
            });
          }
        },
      });
    }
  },

  async removeFiles(folderId: number, hashes: string[], selection?: SelectionQuerySpec) {
    const count = hashes.length;
    if (count > 0) store.set(adjustFolderCountAtom, { folderId, delta: -count });
    eagerGridRemove(folderId, hashes);
    await api.folders.removeFiles(folderId, hashes, selection);
    if (hashes.length > 0 && !selection) {
      registerUndoAction({
        label: `Remove ${hashes.length} item${hashes.length === 1 ? '' : 's'} from folder`,
        backward: async () => {
          await api.folders.addFiles(folderId, hashes);
          store.set(adjustFolderCountAtom, { folderId, delta: count });
          if (useNavigationStore.getState().activeFolderId === folderId) {
            queryApi.file.getGridItems(hashes).then((entities) => {
              if (entities.length > 0) useGridMetadataStore.getState().queueInsertions(entities);
            });
          }
        },
        forward: async () => {
          await api.folders.removeFiles(folderId, hashes);
          store.set(adjustFolderCountAtom, { folderId, delta: -count });
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
    store.set(reorderFolderNodesAtom, moves);
    await api.folders.reorder(moves);
    if (previousMoves) {
      registerUndoAction({
        label: 'Reorder folders',
        backward: async () => { await api.folders.reorder(previousMoves); store.set(reorderFolderNodesAtom, previousMoves); },
        forward: async () => { await api.folders.reorder(moves); store.set(reorderFolderNodesAtom, moves); },
      });
    }
  },

  async move(
    folderId: number,
    newParentId: number | null,
    siblingOrder: [number, number][],
    undoParams?: { oldParentId: number | null; oldSiblingMoves: [number, number][] },
  ) {
    store.set(moveFolderNodeAtom, { folderId, newParentId });
    store.set(reorderFolderNodesAtom, siblingOrder);
    await api.folders.moveFolder(folderId, newParentId, siblingOrder);
    if (undoParams) {
      registerUndoAction({
        label: 'Move folder',
        backward: async () => {
          await api.folders.moveFolder(folderId, undoParams.oldParentId, undoParams.oldSiblingMoves);
          store.set(moveFolderNodeAtom, { folderId, newParentId: undoParams.oldParentId });
          store.set(reorderFolderNodesAtom, undoParams.oldSiblingMoves);
        },
        forward: async () => {
          await api.folders.moveFolder(folderId, newParentId, siblingOrder);
          store.set(moveFolderNodeAtom, { folderId, newParentId });
          store.set(reorderFolderNodesAtom, siblingOrder);
        },
      });
    }
  },

  // ── Icon / color / auto-tags ───────────────────────────────────────────────

  async applyIcon(ids: number[], icon: string | null, previousValues: Array<{ id: number; icon: string | null }>) {
    const value = icon === null ? '' : (icon ?? undefined);
    for (const id of ids) store.set(patchFolderNodeAtom, { folderId: id, patch: { icon } });
    await Promise.all(ids.map((id) => api.folders.update({ folder_id: id, icon: value })));
    registerUndoAction({
      label: ids.length > 1 ? 'Change folder icons' : 'Change folder icon',
      backward: async () => {
        await Promise.all(previousValues.map((e) => api.folders.update({ folder_id: e.id, icon: e.icon === null ? '' : (e.icon ?? undefined) })));
        for (const e of previousValues) store.set(patchFolderNodeAtom, { folderId: e.id, patch: { icon: e.icon } });
      },
      forward: async () => {
        await Promise.all(ids.map((id) => api.folders.update({ folder_id: id, icon: value })));
        for (const id of ids) store.set(patchFolderNodeAtom, { folderId: id, patch: { icon } });
      },
    });
  },

  async applyColor(ids: number[], color: string | null, previousValues: Array<{ id: number; color: string | null }>) {
    const value = color === null ? '' : (color ?? undefined);
    for (const id of ids) store.set(patchFolderNodeAtom, { folderId: id, patch: { color } });
    await Promise.all(ids.map((id) => api.folders.update({ folder_id: id, color: value })));
    registerUndoAction({
      label: ids.length > 1 ? 'Change folder colors' : 'Change folder color',
      backward: async () => {
        await Promise.all(previousValues.map((e) => api.folders.update({ folder_id: e.id, color: e.color === null ? '' : (e.color ?? undefined) })));
        for (const e of previousValues) store.set(patchFolderNodeAtom, { folderId: e.id, patch: { color: e.color } });
      },
      forward: async () => {
        await Promise.all(ids.map((id) => api.folders.update({ folder_id: id, color: value })));
        for (const id of ids) store.set(patchFolderNodeAtom, { folderId: id, patch: { color } });
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
