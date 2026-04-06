import * as api from '../platform/api';

export const collectionsController = {
  async exists(collectionId: number): Promise<boolean> {
    try {
      await api.getCollectionSummary(collectionId);
      return true;
    } catch {
      return false;
    }
  },
};
