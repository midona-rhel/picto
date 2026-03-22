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

  async addToHashes(hashes: string[], tags: string[]) {
    console.log('[tagsController.addToHashes]', { hashes, tags });
    await api.tags.add(hashes, tags);
    console.log('[tagsController.addToHashes] backend succeeded, invalidating metadata');
    filesController.noteManyMetadataChanged(hashes);
    const h = [...hashes], t = [...tags];
    registerUndoAction({
      label: `Add ${t.length} tag${t.length === 1 ? '' : 's'}`,
      backward: async () => { await this.removeFromHashes(h, t); },
      forward: async () => { await this.addToHashes(h, t); },
    });
  },

  async removeFromHashes(hashes: string[], tags: string[]) {
    console.log('[tagsController.removeFromHashes]', { hashes, tags });
    await api.tags.remove(hashes, tags);
    console.log('[tagsController.removeFromHashes] backend succeeded, invalidating metadata');
    filesController.noteManyMetadataChanged(hashes);
    const h = [...hashes], t = [...tags];
    registerUndoAction({
      label: `Remove ${t.length} tag${t.length === 1 ? '' : 's'}`,
      backward: async () => { await this.addToHashes(h, t); },
      forward: async () => { await this.removeFromHashes(h, t); },
    });
  },

  async addToSelection(selection: SelectionQuerySpec, tags: string[]) {
    await api.selection.addTags(selection, tags);
    if (selection.hashes?.length) filesController.noteManyMetadataChanged(selection.hashes);
    const spec = structuredClone(selection), t = [...tags];
    registerUndoAction({
      label: `Add ${t.length} tag${t.length === 1 ? '' : 's'}`,
      backward: async () => { await this.removeFromSelection(spec, t); },
      forward: async () => { await this.addToSelection(spec, t); },
    });
  },

  async removeFromSelection(selection: SelectionQuerySpec, tags: string[]) {
    await api.selection.removeTags(selection, tags);
    if (selection.hashes?.length) filesController.noteManyMetadataChanged(selection.hashes);
    const spec = structuredClone(selection), t = [...tags];
    registerUndoAction({
      label: `Remove ${t.length} tag${t.length === 1 ? '' : 's'}`,
      backward: async () => { await this.addToSelection(spec, t); },
      forward: async () => { await this.removeFromSelection(spec, t); },
    });
  },

  async rename(tagId: number, newName: string, oldName: string): Promise<RenameTagResult> {
    const result = await api.tags.rename(tagId, newName);
    if (!result.merged_into) {
      const { namespace, subtag } = parseTagName(newName);
      useTagListStore.getState().queueRename(tagId, namespace, subtag);
      registerUndoAction({
        label: 'Rename tag',
        backward: async () => { await this.rename(tagId, oldName, newName); },
        forward: async () => { await this.rename(tagId, newName, oldName); },
      });
    } else {
      useTagListStore.getState().queueRemoval(tagId);
    }
    return result;
  },

  async deleteTag(tagId: number, tagDisplay: string, affectedHashes: string[]): Promise<DeleteTagResult> {
    const result = await api.tags.delete(tagId);
    useTagListStore.getState().queueRemoval(tagId);
    const h = [...affectedHashes], display = tagDisplay;
    registerUndoAction({
      label: `Delete tag "${display}"`,
      backward: async () => {
        if (h.length > 0) await this.addToHashes(h, [display]);
      },
      forward: async () => {
        const found = await this.getPaginated({ search: display, limit: 10 });
        const match = found.find((t) => `${t.namespace ? t.namespace + ':' : ''}${t.subtag}` === display);
        if (match) await this.deleteTag(match.tag_id, display, []);
      },
    });
    return result;
  },

  async mergeTag(fromTag: string, toTag: string, sourceHashes: string[], sourceOnlyHashes: string[]) {
    await api.tags.merge(fromTag, toTag);
    const { namespace, subtag } = parseTagName(fromTag);
    useTagListStore.getState().queueRemovalByName(namespace, subtag);
    registerUndoAction({
      label: `Merge tag "${fromTag}" into "${toTag}"`,
      backward: async () => {
        if (sourceHashes.length > 0) await this.addToHashes(sourceHashes, [fromTag]);
        if (sourceOnlyHashes.length > 0) await this.removeFromHashes(sourceOnlyHashes, [toTag]);
      },
      forward: async () => { await this.mergeTag(fromTag, toTag, sourceHashes, sourceOnlyHashes); },
    });
  },

  async setAlias(from: string, to: string) {
    await api.tags.manageAlias(from, to);
    registerUndoAction({
      label: `Set alias "${from}"`,
      backward: async () => { await this.removeAlias(from); },
      forward: async () => { await this.setAlias(from, to); },
    });
  },

  async removeAlias(from: string) {
    await api.tags.manageAlias(from);
    registerUndoAction({
      label: `Remove alias "${from}"`,
      backward: async () => {
        // Cannot restore alias without knowing the original target
      },
      forward: async () => { await this.removeAlias(from); },
    });
  },

  async setImplication(child: string, parent: string, action: 'add' | 'remove') {
    await api.tags.manageImplication(child, parent, action);
    const inverse = action === 'add' ? 'remove' : 'add';
    registerUndoAction({
      label: action === 'add' ? `Add implication "${parent}"` : `Remove implication "${parent}"`,
      backward: async () => { await this.setImplication(child, parent, inverse); },
      forward: async () => { await this.setImplication(child, parent, action); },
    });
  },
};
