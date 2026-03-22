/** Companion panel — tag namespace browsing and file-by-tag queries.
 *  Kept separate: read-only cross-cut that doesn't belong in tagsController. */
import { api } from '#desktop/api';
import type { CompanionNamespaceValue, EntitySlim } from '../shared/types/api';

export const companionController = {
  getNamespaceValues(namespace: string): Promise<CompanionNamespaceValue[]> {
    return api.companion.getNamespaceValues(namespace);
  },

  getFilesByTag(tag: string): Promise<EntitySlim[]> {
    return api.companion.getFilesByTag(tag);
  },
};
