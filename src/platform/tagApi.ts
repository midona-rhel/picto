import { invoke } from './ipc';
import type {
  CanonicalNamespaceSummary,
  MutationReceipt,
  TagPage,
} from '../shared/types/canonical';

export function getTagsPaginated(params: {
  namespace?: string | null;
  search?: string | null;
  cursor?: string | null;
  limit?: number;
}): Promise<TagPage> {
  const input = {
    namespace: params.namespace ?? null,
    search: params.search ?? null,
    cursor: params.cursor ?? null,
    limit: params.limit ?? 100,
  };
  return invoke<TagPage>('tags.list', input);
}

export function getNamespaceSummary(): Promise<CanonicalNamespaceSummary[]> {
  return invoke<CanonicalNamespaceSummary[]>('tags.namespace_counts');
}

export function getTagsById(tagIds: number[]): Promise<TagPage['tags']> {
  return invoke<TagPage['tags']>('tags.get_many', { tag_ids: tagIds });
}

export function getUnusedTagCount(): Promise<number> {
  return invoke<number>('tags.unused_count');
}

export function renameTag(tagId: number, newName: string): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('tags.rename_or_merge', { tag_id: tagId, name: newName });
}

export function mergeTags(fromTagId: number, toTagName: string): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('tags.rename_or_merge', { tag_id: fromTagId, name: toTagName });
}

export function deleteTag(tagId: number): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('tags.delete', { tag_id: tagId });
}

export function deleteUnusedTags(): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('tags.delete_unused', {});
}

export function createTagGroup(name: string): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('tags.group.create', { name });
}

export function renameTagGroup(namespaceId: number, name: string): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('tags.group.rename', { namespace_id: namespaceId, name });
}

export function deleteTagGroup(namespaceId: number): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('tags.group.delete', { namespace_id: namespaceId });
}
