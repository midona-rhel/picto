import {
  addCollectionMembers,
  createCollection,
  getCollectionSummary,
  removeCollectionMembers,
  splitCollection,
} from '../platform/collectionApi';

export const collectionsController = {
  create(name: string, hashes: string[]): Promise<number> {
    return createCollection(name, hashes);
  },

  addMembers(collectionId: number, hashes: string[]): Promise<number> {
    return addCollectionMembers(collectionId, hashes);
  },

  removeMembers(collectionId: number, hashes: string[]): Promise<number> {
    return removeCollectionMembers(collectionId, hashes);
  },

  split(collectionId: number): Promise<string[]> {
    return splitCollection(collectionId);
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
