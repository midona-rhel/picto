import { getNamespaceSummary, getTagsPaginated, searchTags } from '../platform/tagApi';
import type { CanonicalNamespaceSummary, CanonicalTagRecord } from '../shared/types/canonical';

export const tagsController = {
  search(query: string, limit = 50): Promise<CanonicalTagRecord[]> {
    return searchTags(query, limit);
  },

  getPaginated(params: {
    namespace?: string | null;
    search?: string | null;
    cursor?: string | null;
    limit?: number;
  }): Promise<CanonicalTagRecord[]> {
    return getTagsPaginated(params);
  },

  getNamespaceSummary(): Promise<CanonicalNamespaceSummary[]> {
    return getNamespaceSummary();
  },
};
