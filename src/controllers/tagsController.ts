import {
  deleteTag,
  getNamespaceSummary,
  getTagRelations,
  getTagsPaginated,
  manageTagAlias,
  manageTagImplication,
  mergeTags,
  renameTag,
} from '../platform/tagApi';
import type {
  CanonicalNamespaceSummary,
  CanonicalTagRelation,
  TagPage,
} from '../shared/types/canonical';

export const tagsController = {
  getPaginated(params: {
    namespace?: string | null;
    search?: string | null;
    cursor?: string | null;
    limit?: number;
  }): Promise<TagPage> {
    return getTagsPaginated(params);
  },

  getNamespaceSummary(): Promise<CanonicalNamespaceSummary[]> {
    return getNamespaceSummary();
  },

  getRelations(tagId: number, relationType: 'aliases' | 'implications'): Promise<CanonicalTagRelation[]> {
    return getTagRelations(tagId, relationType);
  },

  rename(tagId: number, newName: string): Promise<unknown> {
    return renameTag(tagId, newName);
  },

  merge(fromTag: string, toTag: string): Promise<unknown> {
    return mergeTags(fromTag, toTag);
  },

  delete(tagId: number): Promise<unknown> {
    return deleteTag(tagId);
  },

  setAlias(from: string, to?: string | null): Promise<void> {
    return manageTagAlias(from, to);
  },

  setImplication(child: string, parent: string, action: 'add' | 'remove'): Promise<void> {
    return manageTagImplication(child, parent, action);
  },
};
