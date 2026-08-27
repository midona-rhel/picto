import {
  deleteTag,
  deleteUnusedTags,
  deleteTagGroup,
  getNamespaceSummary,
  getUnusedTagCount,
  getTagsPaginated,
  mergeTags,
  renameTagGroup,
  renameTag,
} from '../platform/tagApi';
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

  async renameGroup(namespace: string, newNamespace: string): Promise<unknown> {
    const result = await renameTagGroup(namespace, newNamespace);
    await announceUndoableMutation('tags.group.rename');
    return result;
  },

  async deleteGroup(namespace: string): Promise<unknown> {
    const result = await deleteTagGroup(namespace);
    await announceUndoableMutation('tags.group.delete');
    return result;
  },

};
