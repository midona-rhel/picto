import { invoke } from './ipc';
import type { ListTagsInput } from '../shared/types/generated/application/ListTagsInput';
import type { MutationReceipt } from '../shared/types/generated/application/MutationReceipt';
import type { RenameTagInput } from '../shared/types/generated/application/RenameTagInput';
import type { RenameTagGroupInput } from '../shared/types/generated/application/RenameTagGroupInput';
import type { TagInput } from '../shared/types/generated/application/TagInput';
import type { TagGroupInput } from '../shared/types/generated/application/TagGroupInput';
import type { TagPage as ReplacementTagPage } from '../shared/types/generated/application/TagPage';
import type { CanonicalNamespaceSummary, TagPage } from '../shared/types/canonical';

type NamespaceCountRows = Array<[string, number]>;

function toTagRecord(tag: ReplacementTagPage['tags'][number]) {
  return {
    tag_id: tag.tag_id,
    namespace: tag.namespace,
    subtag: tag.subtag,
    file_count: tag.media_count,
  };
}

export function getTagsPaginated(params: {
  namespace?: string | null;
  search?: string | null;
  cursor?: string | null;
  limit?: number;
}): Promise<TagPage> {
  const input: ListTagsInput = {
    namespace: params.namespace ?? null,
    search: params.search ?? null,
    cursor: params.cursor ?? null,
    limit: params.limit ?? 100,
  };
  return invoke<ReplacementTagPage>('tags.list', input).then((page) => ({
    items: page.tags.map(toTagRecord),
    next_cursor: page.next_cursor,
  }));
}

export function getNamespaceSummary(): Promise<CanonicalNamespaceSummary[]> {
  return invoke<NamespaceCountRows>('tags.namespace_counts').then((rows) =>
    rows.map(([namespace, count]) => ({ namespace, count })),
  );
}

export function getUnusedTagCount(): Promise<number> {
  return invoke<number>('tags.unused_count');
}

export function renameTag(tagId: number, newName: string): Promise<MutationReceipt> {
  const input: RenameTagInput = { tag_id: tagId, name: newName };
  return invoke<MutationReceipt>('tags.rename_or_merge', input);
}

export function mergeTags(fromTagId: number, toTagName: string): Promise<MutationReceipt> {
  const input: RenameTagInput = { tag_id: fromTagId, name: toTagName };
  return invoke<MutationReceipt>('tags.rename_or_merge', input);
}

export function deleteTag(tagId: number): Promise<MutationReceipt> {
  const input: TagInput = { tag_id: tagId };
  return invoke<MutationReceipt>('tags.delete', input);
}

export function deleteUnusedTags(): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('tags.delete_unused', {});
}

export function renameTagGroup(namespace: string, newNamespace: string): Promise<MutationReceipt> {
  const input: RenameTagGroupInput = { namespace, new_namespace: newNamespace };
  return invoke<MutationReceipt>('tags.group.rename', input);
}

export function deleteTagGroup(namespace: string): Promise<MutationReceipt> {
  const input: TagGroupInput = { namespace };
  return invoke<MutationReceipt>('tags.group.delete', input);
}
