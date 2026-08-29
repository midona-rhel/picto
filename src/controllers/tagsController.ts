import {
  createTagGroup,
  deleteTag,
  deleteUnusedTags,
  deleteTagGroup,
  getNamespaceSummary,
  getUnusedTagCount,
  getTagsPaginated,
  getTagsById,
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

  getById(tagIds: number[]) {
    return getTagsById(tagIds);
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

  async renameGroup(namespaceId: number, name: string): Promise<unknown> {
    const result = await renameTagGroup(namespaceId, name);
    await announceUndoableMutation('tags.group.rename');
    return result;
  },

  async createGroup(name: string): Promise<unknown> {
    const result = await createTagGroup(name);
    await announceUndoableMutation('tags.group.create');
    return result;
  },

  async deleteGroup(namespaceId: number): Promise<unknown> {
    const result = await deleteTagGroup(namespaceId);
    await announceUndoableMutation('tags.group.delete');
    return result;
  },

};
