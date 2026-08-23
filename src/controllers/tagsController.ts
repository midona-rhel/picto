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
import type { TagRelationGroups } from '../platform/tagApi';
import type {
  CanonicalNamespaceSummary,
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

  getRelations(tagId: number): Promise<TagRelationGroups> {
    return getTagRelations(tagId);
  },

  rename(tagId: number, newName: string): Promise<unknown> {
    return renameTag(tagId, newName);
  },

  merge(fromTagId: number, toTagName: string): Promise<unknown> {
    return mergeTags(fromTagId, toTagName);
  },

  delete(tagId: number): Promise<unknown> {
    return deleteTag(tagId);
  },

  setAlias(fromTagId: number, toTagId?: number | null): Promise<void> {
    return manageTagAlias(fromTagId, toTagId).then(() => undefined);
  },

  setImplication(childTagId: number, parentTagId: number, present: boolean): Promise<void> {
    return manageTagImplication(childTagId, parentTagId, present).then(() => undefined);
  },
};
