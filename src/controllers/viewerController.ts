import { getEntityDetails } from '../platform/entityApi';
import type { CanonicalEntityDetails } from '../shared/types/canonical';

export const viewerController = {
  getEntityDetails(entityHash: string): Promise<CanonicalEntityDetails | null> {
    return getEntityDetails(entityHash);
  },
};
