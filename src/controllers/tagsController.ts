import { api } from '#desktop/api';
import { filesController } from './filesController';
import { registerUndoAction } from '../shared/controllers/undoRedoController';
import { useTagListStore } from '../state/tagListStore';
import type {
  DeleteTagResult,
  NamespaceSummary,
  RenameTagResult,
  SelectionQuerySpec,
  TagDisplay,
  TagRecord,
  TagRelation,
} from '../shared/types/api';

function parseTagName(fullName: string): { namespace: string; subtag: string } {
  const idx = fullName.indexOf(':');
  if (idx === -1) return { namespace: '', subtag: fullName };
  return { namespace: fullName.slice(0, idx), subtag: fullName.slice(idx + 1) };
}

export const tagsController = {
  // ── Reads ──

  search(query: string, limit?: number) {
    return api.tags.search(query, limit);
  },

  getAll() {
    return api.tags.getAll();
  },

  getForFile(hash: string): Promise<TagDisplay[]> {
    return api.tags.getForFile(hash);
  },

  getPaginated(params: { namespace?: string; search?: string; cursor?: string; limit?: number }): Promise<TagRecord[]> {
    return api.tags.getPaginated(params);
  },

  getNamespaceSummary(): Promise<NamespaceSummary[]> {
    return api.tags.getNamespaceSummary();
  },

  findFilesByTags(tagStrings: string[], limit?: number, offset?: number): Promise<string[]> {
    return api.tags.findFilesByTags(tagStrings, limit, offset);
  },

  getRelations(tagId: number, relationType: 'aliases' | 'implications'): Promise<TagRelation[]> {
    return api.tags.getRelations(tagId, relationType);
  },

  // ── Tag management writes ──

  /** @internal */ async _rename(tagId: number, newName: string): Promise<RenameTagResult> {
    const result = await api.tags.rename(tagId, newName);
    if (!result.merged_into) {
      const { namespace, subtag } = parseTagName(newName);
      useTagListStore.getState().queueRename(tagId, namespace, subtag);
    } else {
      useTagListStore.getState().queueRemoval(tagId);
    }
    return result;
  },
  /** @internal */ async _delete(tagId: number): Promise<DeleteTagResult> {
    const result = await api.tags.delete(tagId);
    useTagListStore.getState().queueRemoval(tagId);
    return result;
  },
  /** @internal */ async _merge(fromTag: string, toTag: string) {
    const result = await api.tags.merge(fromTag, toTag);
    const { namespace, subtag } = parseTagName(fromTag);
    useTagListStore.getState().queueRemovalByName(namespace, subtag);
    return result;
  },
  /** @internal */ async _manageAlias(from: string, to?: string) {
    return api.tags.manageAlias(from, to);
  },
  /** @internal */ async _manageImplication(child: string, parent: string, action: 'add' | 'remove') {
    return api.tags.manageImplication(child, parent, action);
  },

  // ── Per-file tag writes (internal — used by undo closures) ──

  /** @internal */ async _addToHashes(hashes: string[], tags: string[]) {
    const result = await api.tags.add(hashes, tags);
    filesController.noteManyMetadataChanged(hashes);
    return result;
  },
  /** @internal */ async _removeFromHashes(hashes: string[], tags: string[]) {
    const result = await api.tags.remove(hashes, tags);
    filesController.noteManyMetadataChanged(hashes);
    return result;
  },
  /** @internal */ async _addToSelection(selection: SelectionQuerySpec, tags: string[]) {
    const result = await api.selection.addTags(selection, tags);
    if (selection.hashes?.length) filesController.noteManyMetadataChanged(selection.hashes);
    return result;
  },
  /** @internal */ async _removeFromSelection(selection: SelectionQuerySpec, tags: string[]) {
    const result = await api.selection.removeTags(selection, tags);
    if (selection.hashes?.length) filesController.noteManyMetadataChanged(selection.hashes);
    return result;
  },

  // ── Public tag write methods (own undo) ──

  async addToHashes(hashes: string[], tags: string[]) {
    await this._addToHashes(hashes, tags);
    const h = [...hashes], t = [...tags];
    registerUndoAction({
      label: `Add ${t.length} tag${t.length === 1 ? '' : 's'}`,
      backward: async () => { await this._removeFromHashes(h, t); },
      forward: async () => { await this._addToHashes(h, t); },
    });
  },

  async removeFromHashes(hashes: string[], tags: string[]) {
    await this._removeFromHashes(hashes, tags);
    const h = [...hashes], t = [...tags];
    registerUndoAction({
      label: `Remove ${t.length} tag${t.length === 1 ? '' : 's'}`,
      backward: async () => { await this._addToHashes(h, t); },
      forward: async () => { await this._removeFromHashes(h, t); },
    });
  },

  async addToSelection(selection: SelectionQuerySpec, tags: string[]) {
    await this._addToSelection(selection, tags);
    const spec = structuredClone(selection), t = [...tags];
    registerUndoAction({
      label: `Add ${t.length} tag${t.length === 1 ? '' : 's'}`,
      backward: async () => { await this._removeFromSelection(spec, t); },
      forward: async () => { await this._addToSelection(spec, t); },
    });
  },

  async removeFromSelection(selection: SelectionQuerySpec, tags: string[]) {
    await this._removeFromSelection(selection, tags);
    const spec = structuredClone(selection), t = [...tags];
    registerUndoAction({
      label: `Remove ${t.length} tag${t.length === 1 ? '' : 's'}`,
      backward: async () => { await this._addToSelection(spec, t); },
      forward: async () => { await this._removeFromSelection(spec, t); },
    });
  },

  async rename(tagId: number, newName: string, oldName: string): Promise<RenameTagResult> {
    const result = await this._rename(tagId, newName);
    if (!result.merged_into) {
      registerUndoAction({
        label: 'Rename tag',
        backward: async () => { await this._rename(tagId, oldName); },
        forward: async () => { await this._rename(tagId, newName); },
      });
    }
    return result;
  },

  async deleteTag(tagId: number, tagDisplay: string, affectedHashes: string[]): Promise<DeleteTagResult> {
    const result = await this._delete(tagId);
    const h = [...affectedHashes], display = tagDisplay;
    registerUndoAction({
      label: `Delete tag "${display}"`,
      backward: async () => {
        if (h.length > 0) await this._addToHashes(h, [display]);
      },
      forward: async () => {
        const found = await this.getPaginated({ search: display, limit: 10 });
        const match = found.find((t) => `${t.namespace ? t.namespace + ':' : ''}${t.subtag}` === display);
        if (match) await this._delete(match.tag_id);
      },
    });
    return result;
  },

  async mergeTag(fromTag: string, toTag: string, sourceHashes: string[], sourceOnlyHashes: string[]) {
    await this._merge(fromTag, toTag);
    registerUndoAction({
      label: `Merge tag "${fromTag}" into "${toTag}"`,
      backward: async () => {
        if (sourceHashes.length > 0) await this._addToHashes(sourceHashes, [fromTag]);
        if (sourceOnlyHashes.length > 0) await this._removeFromHashes(sourceOnlyHashes, [toTag]);
      },
      forward: async () => { await this._merge(fromTag, toTag); },
    });
  },

  async setAlias(from: string, to: string) {
    await this._manageAlias(from, to);
    registerUndoAction({
      label: `Set alias "${from}"`,
      backward: async () => { await this._manageAlias(from); },
      forward: async () => { await this._manageAlias(from, to); },
    });
  },

  async setImplication(child: string, parent: string, action: 'add' | 'remove') {
    await this._manageImplication(child, parent, action);
    const inverse = action === 'add' ? 'remove' : 'add';
    registerUndoAction({
      label: action === 'add' ? `Add implication "${parent}"` : `Remove implication "${parent}"`,
      backward: async () => { await this._manageImplication(child, parent, inverse); },
      forward: async () => { await this._manageImplication(child, parent, action); },
    });
  },
};
