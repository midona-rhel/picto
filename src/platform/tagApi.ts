import { invoke } from './ipc';
import type { CanonicalNamespaceSummary, CanonicalTagRelation, TagPage } from '../shared/types/canonical';

export function getTagsPaginated(params: {
  namespace?: string | null;
  search?: string | null;
  cursor?: string | null;
  limit?: number;
}): Promise<TagPage> {
  return invoke<TagPage>('get_tags_paginated', params as unknown as Record<string, unknown>);
}

export function getNamespaceSummary(): Promise<CanonicalNamespaceSummary[]> {
  return invoke<CanonicalNamespaceSummary[]>('get_namespace_summary');
}

export function getTagRelations(tagId: number, relationType: 'aliases' | 'implications'): Promise<CanonicalTagRelation[]> {
  return invoke<CanonicalTagRelation[]>('get_tag_relations', {
    tag_id: tagId,
    relation_type: relationType,
  });
}

export function renameTag(tagId: number, newName: string): Promise<unknown> {
  return invoke('rename_tag', { tag_id: tagId, new_name: newName });
}

export function mergeTags(fromTag: string, toTag: string): Promise<unknown> {
  return invoke('merge_tags', { from_tag: fromTag, to_tag: toTag });
}

export function deleteTag(tagId: number): Promise<unknown> {
  return invoke('delete_tag', { tag_id: tagId });
}

export function manageTagAlias(from: string, to?: string | null): Promise<void> {
  return invoke<void>('manage_tag_alias', { from, to: to ?? null });
}

export function manageTagImplication(
  child: string,
  parent: string,
  action: 'add' | 'remove',
): Promise<void> {
  return invoke<void>('manage_tag_implication', { child, parent, action });
}
