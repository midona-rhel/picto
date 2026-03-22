import { api } from '#desktop/api';
import { filesController } from './filesController';
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

  async rename(tagId: number, newName: string): Promise<RenameTagResult> {
    const result = await api.tags.rename(tagId, newName);
    if (!result.merged_into) {
      const { namespace, subtag } = parseTagName(newName);
      useTagListStore.getState().queueRename(tagId, namespace, subtag);
    } else {
      // Rename caused a merge — source tag is gone
      useTagListStore.getState().queueRemoval(tagId);
    }
    return result;
  },

  async delete(tagId: number): Promise<DeleteTagResult> {
    const result = await api.tags.delete(tagId);
    useTagListStore.getState().queueRemoval(tagId);
    return result;
  },

  async merge(fromTag: string, toTag: string) {
    const result = await api.tags.merge(fromTag, toTag);
    // Source tag is absorbed — remove it from the visible list.
    const { namespace, subtag } = parseTagName(fromTag);
    useTagListStore.getState().queueRemovalByName(namespace, subtag);
    return result;
  },

  async manageAlias(from: string, to?: string) {
    return api.tags.manageAlias(from, to);
  },

  async manageImplication(child: string, parent: string, action: 'add' | 'remove') {
    return api.tags.manageImplication(child, parent, action);
  },

  // ── Per-file tag writes (eagerly invalidate affected metadata) ──

  async addToHashes(hashes: string[], tags: string[]) {
    const result = await api.tags.add(hashes, tags);
    filesController.noteManyMetadataChanged(hashes);
    return result;
  },

  async removeFromHashes(hashes: string[], tags: string[]) {
    const result = await api.tags.remove(hashes, tags);
    filesController.noteManyMetadataChanged(hashes);
    return result;
  },

  async addToSelection(selection: SelectionQuerySpec, tags: string[]) {
    const result = await api.selection.addTags(selection, tags);
    if (selection.hashes?.length) {
      filesController.noteManyMetadataChanged(selection.hashes);
    }
    return result;
  },

  async removeFromSelection(selection: SelectionQuerySpec, tags: string[]) {
    const result = await api.selection.removeTags(selection, tags);
    if (selection.hashes?.length) {
      filesController.noteManyMetadataChanged(selection.hashes);
    }
    return result;
  },
};
