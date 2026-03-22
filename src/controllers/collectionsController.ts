import { api } from '#desktop/api';
import { filesController } from './filesController';
import { registerUndoAction } from '../shared/controllers/undoRedoController';
import { useCollectionListStore } from '../state/collectionListStore';
import { useGridMetadataStore } from '../state/gridMetadataStore';
import type { CollectionSummary, CollectionInfo } from '../shared/types/api';

// ── Smart naming helpers ──

const GENERATED_NAME_RE = /^(?:[a-f0-9]{24,}|image[_-]?\d+|img[_-]?\d+|file[_-]?\d+)$/i;

function isGeneratedName(name: string): boolean {
  return GENERATED_NAME_RE.test(name.trim());
}

function normalizeNameBase(name: string): string {
  return name
    .trim()
    .replace(/\.[a-z0-9]{2,5}$/i, '')
    .replace(/(?:[\s._-]|\s*\(\s*)\d+\s*\)?$/g, '')
    .trim()
    .toLowerCase();
}

function inferCollectionName(memberNames: string[]): string {
  const now = new Date();
  const fallback = `Collection ${now.toLocaleDateString()} ${now.toLocaleTimeString()}`;
  const names = memberNames.map((n) => n.trim()).filter(Boolean);
  if (names.length === 0) return fallback;

  const allGenerated = names.every(isGeneratedName);
  const bases = names.map(normalizeNameBase).filter(Boolean);
  const uniqueBases = new Set(bases);

  if (uniqueBases.size === 1 && bases.length > 0) {
    return names.find((n) => normalizeNameBase(n) === bases[0]) ?? fallback;
  }
  if (!allGenerated) return names[0];
  return fallback;
}

// ── Eager grid helpers ──

async function eagerInsertIfViewingCollection(collectionId: number, hashes: string[]) {
  const scope = useGridMetadataStore.getState().activeGridScope;
  if (scope !== `collection:${collectionId}`) return;
  const { queryApi } = await import('#desktop/queryApi');
  const entities = await Promise.all(hashes.map((h) => queryApi.file.get(h)));
  const valid = entities.filter((e): e is NonNullable<typeof e> => e != null);
  if (valid.length > 0) useGridMetadataStore.getState().queueInsertions(valid);
}

function eagerRemoveIfViewingCollection(collectionId: number, hashes: string[]) {
  const scope = useGridMetadataStore.getState().activeGridScope;
  if (scope === `collection:${collectionId}`) {
    useGridMetadataStore.getState().queueRemovals(hashes);
  }
}

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


  // ── Atomic writes ──

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
    void eagerInsertIfViewingCollection(params.id, params.hashes);
    return result;
  },

  async removeMembers(params: { id: number; hashes: string[] }) {
    const result = await api.collections.removeMembers(params);
    eagerRemoveIfViewingCollection(params.id, params.hashes);
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

  // ── Workflow methods (own backend calls + eager consequences + undo) ──

  /** Create a collection from selected images with smart name inference. */
  async createFromSelection(
    images: Array<{ hash: string; name: string | null }>,
  ): Promise<{ id: number; name: string; count: number }> {
    const hashes = images.map((img) => img.hash);
    const name = inferCollectionName(images.map((img) => img.name ?? ''));
    const id = await this.create({ name });
    const count = await this.addMembers({ id, hashes });
    registerUndoAction({
      label: `Create collection "${name}"`,
      backward: async () => { await this.delete(id); },
      forward: async () => {
        const newId = await this.create({ name });
        await this.addMembers({ id: newId, hashes });
      },
    });
    return { id, name, count: Number(count ?? hashes.length) };
  },

  /** Merge loose images into an existing collection. */
  async mergeInto(
    targetId: number,
    hashes: string[],
  ): Promise<number> {
    const count = Number(await this.addMembers({ id: targetId, hashes }) ?? hashes.length);
    registerUndoAction({
      label: `Add ${count} item${count === 1 ? '' : 's'} to collection`,
      backward: async () => { await this.removeMembers({ id: targetId, hashes }); },
      forward: async () => { await this.addMembers({ id: targetId, hashes }); },
    });
    return count;
  },

  /** Merge multiple collections (and optional loose singles) into the target. */
  async mergeCollections(
    target: { entity_id: number; name: string },
    others: Array<{ entity_id: number }>,
    looseSingleHashes: string[],
  ): Promise<void> {
    const targetId = target.entity_id;
    for (const other of others) {
      const memberHashes = await this.listMemberHashes(other.entity_id);
      if (memberHashes.length > 0) {
        await this.addMembers({ id: targetId, hashes: memberHashes });
      }
      await this.delete(other.entity_id);
    }
    if (looseSingleHashes.length > 0) {
      await this.addMembers({ id: targetId, hashes: looseSingleHashes });
    }
  },

  /** Split a collection: delete it and return its former member hashes. */
  async split(
    collectionId: number,
    collectionName: string,
  ): Promise<string[]> {
    const memberHashes = await this.listMemberHashes(collectionId);
    await this.delete(collectionId);
    registerUndoAction({
      label: `Split collection "${collectionName}"`,
      backward: async () => {
        const newId = await this.create({ name: collectionName });
        if (memberHashes.length > 0) {
          await this.addMembers({ id: newId, hashes: memberHashes });
        }
      },
      forward: async () => {
        // Re-split: find the restored collection and delete it again
        const list = await this.list();
        const match = list.find((c) => c.name === collectionName);
        if (match) await this.delete(match.id);
      },
    });
    return memberHashes;
  },

  /** Remove a single member from a collection with undo. */
  async removeMember(collectionId: number, hash: string): Promise<void> {
    await this.removeMembers({ id: collectionId, hashes: [hash] });
    registerUndoAction({
      label: 'Remove from collection',
      backward: async () => { await this.addMembers({ id: collectionId, hashes: [hash] }); },
      forward: async () => { await this.removeMembers({ id: collectionId, hashes: [hash] }); },
    });
  },

  /** Remove multiple members from a collection with undo. */
  async removeMembersBatch(collectionId: number, hashes: string[]): Promise<void> {
    const h = [...hashes];
    await this.removeMembers({ id: collectionId, hashes: h });
    registerUndoAction({
      label: `Remove ${h.length} from collection`,
      backward: async () => { await this.addMembers({ id: collectionId, hashes: h }); },
      forward: async () => { await this.removeMembers({ id: collectionId, hashes: h }); },
    });
  },

  /** Rename a collection with undo. */
  async rename(id: number, newName: string, oldName: string): Promise<void> {
    await this.update({ id, name: newName });
    registerUndoAction({
      label: 'Rename collection',
      backward: async () => { await this.update({ id, name: oldName }); },
      forward: async () => { await this.update({ id, name: newName }); },
    });
  },

  /** Add tags to a collection with undo. */
  async tagAdd(id: number, tags: string[]): Promise<void> {
    const t = [...tags];
    await this.addTags(id, t);
    registerUndoAction({
      label: `Add ${t.length} tag${t.length === 1 ? '' : 's'}`,
      backward: async () => { await this.removeTags(id, t); },
      forward: async () => { await this.addTags(id, t); },
    });
  },

  /** Remove tags from a collection with undo. */
  async tagRemove(id: number, tags: string[]): Promise<void> {
    const t = [...tags];
    await this.removeTags(id, t);
    registerUndoAction({
      label: `Remove ${t.length} tag${t.length === 1 ? '' : 's'}`,
      backward: async () => { await this.addTags(id, t); },
      forward: async () => { await this.removeTags(id, t); },
    });
  },
};
