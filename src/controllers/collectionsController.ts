import {
  addCollectionMembers,
  createCollection,
  deleteCollection,
  getCollectionSummary,
  listCollectionMemberHashes,
  removeCollectionMembers,
} from '../platform/collectionApi';

export const collectionsController = {
  create(name: string): Promise<number> {
    return createCollection(name);
  },

  addMembers(collectionId: number, hashes: string[]): Promise<number> {
    return addCollectionMembers(collectionId, hashes);
  },

  removeMembers(collectionId: number, hashes: string[]): Promise<number> {
    return removeCollectionMembers(collectionId, hashes);
  },

  delete(collectionId: number): Promise<void> {
    return deleteCollection(collectionId);
  },

  listMemberHashes(collectionId: number): Promise<string[]> {
    return listCollectionMemberHashes(collectionId);
  },

  async exists(collectionId: number): Promise<boolean> {
    try {
      await getCollectionSummary(collectionId);
      return true;
    } catch {
      return false;
    }
  },
};
