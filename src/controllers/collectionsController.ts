import { api } from '#desktop/api';
import { filesController } from './filesController';
import { useCollectionListStore } from '../state/collectionListStore';
import { useGridMetadataStore } from '../state/gridMetadataStore';
import type { CollectionSummary, CollectionInfo } from '../shared/types/api';

export const collectionsController = {
  // ── Reads ──

  list(): Promise<CollectionInfo[]> {
    return api.collections.list();
  },

  getSummary(id: number): Promise<CollectionSummary> {
    return api.collections.getSummary(id);
  },

  listMemberHashes(id: number): Promise<string[]> {
    return api.collections.listMemberHashes(id);
  },

  // ── Writes ──

  async create(params: { name: string }) {
    const id = await api.collections.create(params);
    if (typeof id === 'number') {
      useCollectionListStore.getState().queueCreated(id);
    }
    return id;
  },

  async delete(id: number) {
    const result = await api.collections.delete(id);
    useCollectionListStore.getState().queueRemoval(id);
    return result;
  },

  async update(params: { id: number; name?: string; tags?: string[] }) {
    const result = await api.collections.update(params);
    if (params.name != null) {
      useCollectionListStore.getState().queueUpdate(params.id, params.name);
    }
    return result;
  },

  async addMembers(params: { id: number; hashes: string[] }) {
    const result = await api.collections.addMembers(params);
    filesController.noteManyMetadataChanged(params.hashes);
    // If viewing this collection, insert the new members into the visible grid.
    const scope = useGridMetadataStore.getState().activeGridScope;
    if (scope === `collection:${params.id}`) {
      const { queryApi } = await import('#desktop/queryApi');
      Promise.all(params.hashes.map((h) => queryApi.file.get(h))).then((entities) => {
        const valid = entities.filter((e): e is NonNullable<typeof e> => e != null);
        if (valid.length > 0) useGridMetadataStore.getState().queueInsertions(valid);
      });
    }
    return result;
  },

  async removeMembers(params: { id: number; hashes: string[] }) {
    const result = await api.collections.removeMembers(params);
    // Members leaving the collection scope — remove from visible grid.
    const scope = useGridMetadataStore.getState().activeGridScope;
    if (scope === `collection:${params.id}`) {
      useGridMetadataStore.getState().queueRemovals(params.hashes);
    }
    filesController.noteManyMetadataChanged(params.hashes);
    return result;
  },

  async reorderMembers(id: number, hashes: string[]) {
    return api.collections.reorderMembers(id, hashes);
  },

  async addTags(id: number, tags: string[]) {
    return api.collections.addTags(id, tags);
  },

  async removeTags(id: number, tags: string[]) {
    return api.collections.removeTags(id, tags);
  },
};
