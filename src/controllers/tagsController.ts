import {
  deleteTag,
  deleteUnusedTags,
  getNamespaceSummary,
  getUnusedTagCount,
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
import { announceUndoableMutation } from '../runtime/historyRuntime';

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

  getUnusedCount(): Promise<number> {
    return getUnusedTagCount();
  },

  getRelations(tagId: number): Promise<TagRelationGroups> {
    return getTagRelations(tagId);
  },

  async rename(tagId: number, newName: string): Promise<unknown> {
    const result = await renameTag(tagId, newName);
    await announceUndoableMutation('tags.rename_or_merge');
    return result;
  },

  async merge(fromTagId: number, toTagName: string): Promise<unknown> {
    const result = await mergeTags(fromTagId, toTagName);
    await announceUndoableMutation('tags.rename_or_merge');
    return result;
  },

  async delete(tagId: number): Promise<unknown> {
    const result = await deleteTag(tagId);
    await announceUndoableMutation('tags.delete');
    return result;
  },

  async deleteUnused(): Promise<unknown> {
    const result = await deleteUnusedTags();
    await announceUndoableMutation('tags.delete_unused');
    return result;
  },

  async setAlias(fromTagId: number, toTagId?: number | null): Promise<void> {
    await manageTagAlias(fromTagId, toTagId);
    await announceUndoableMutation('tags.set_alias');
  },

  async setImplication(childTagId: number, parentTagId: number, present: boolean): Promise<void> {
    await manageTagImplication(childTagId, parentTagId, present);
    await announceUndoableMutation('tags.set_implication');
  },
};
