import { invoke } from './ipc';
import type {
  DetachCollectionInput,
  EntityTarget,
  EntityViewPage,
  EntityViewQuery,
  Lifecycle,
  MediaEntityPatch,
  MutationReceipt,
  OrganizeCollectionInput,
  CollectionNoteDraft,
  OrganizeCollectionResult,
  QueryPage,
  ReorderCollectionInput,
  SelectionSummary,
} from '../shared/types/canonical';

export function queryItems(query: EntityViewQuery, page: QueryPage): Promise<EntityViewPage> {
  return invoke<EntityViewPage>('items.query', { query, page });
}

export function recordMediaView(rootId: number): Promise<unknown> {
  return invoke('items.record_view', { root_id: rootId });
}

export function clearRecentViews(): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.clear_recent_views', {});
}

export function renameItem(rootId: number, name: string): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.rename', { root_id: rootId, name });
}

export function renameItems(renames: Array<{ root_id: number; name: string }>): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.rename_many', { renames });
}

export function patchMediaEntities(target: EntityTarget, patch: MediaEntityPatch): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.patch_metadata', { target, patch });
}

export function applyEntityTags(
  target: EntityTarget,
  operation: 'add' | 'remove',
  tags: string[],
): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.apply_tags', {
    target,
    tags,
    add: operation === 'add',
  });
}

export function setItemLifecycle(target: EntityTarget, lifecycle: Lifecycle): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.set_lifecycle', { target, lifecycle });
}

export function deleteItems(target: EntityTarget): Promise<unknown> {
  return invoke('items.delete', { target });
}

export function getSelectionSummary(target: EntityTarget): Promise<SelectionSummary> {
  return invoke<SelectionSummary>('items.selection_summary', { target });
}

export function resolveImageSelection(target: EntityTarget): Promise<number[]> {
  return invoke<number[]>('items.resolve_image_selection', { target });
}

export function getCollectionNoteDraft(target: EntityTarget): Promise<CollectionNoteDraft> {
  return invoke<CollectionNoteDraft>('items.collection_note_draft', { target });
}

export function organizeIntoGroup(input: OrganizeCollectionInput): Promise<OrganizeCollectionResult> {
  return invoke<OrganizeCollectionResult>('items.organize_into_collection', { ...input });
}

export function detachItems(input: DetachCollectionInput): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.detach', { ...input });
}

export function ungroup(collectionId: number): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.ungroup', { collection_id: collectionId });
}

export function reorderGroup(input: ReorderCollectionInput): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.reorder_collection', { ...input });
}
