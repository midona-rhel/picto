import { getEntityDetails, queryEntityView } from '../platform/entityApi';
import type { CanonicalEntityDetails, EntityViewPage, EntityViewQuery } from '../shared/types/canonical';

export const viewerController = {
  getEntityDetails(entityHash: string): Promise<CanonicalEntityDetails | null> {
    return getEntityDetails(entityHash);
  },

  queryEntityView(query: EntityViewQuery): Promise<EntityViewPage> {
    return queryEntityView(query);
  },
};
