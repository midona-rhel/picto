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
