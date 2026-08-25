import { invoke } from './ipc';
import type { ItemPage } from '../shared/types/generated/application/ItemPage';
import type { ItemPageRequest } from '../shared/types/generated/application/ItemPageRequest';
import type { ItemQuery } from '../shared/types/generated/application/ItemQuery';
import type { ItemTarget } from '../shared/types/generated/application/ItemTarget';
import type { DetachItemsInput } from '../shared/types/generated/application/DetachItemsInput';
import type { OrganizeIntoCollectionInput } from '../shared/types/generated/application/OrganizeIntoCollectionInput';
import type { OrganizeIntoCollectionResult } from '../shared/types/generated/application/OrganizeIntoCollectionResult';
import type { Lifecycle } from '../shared/types/generated/application/Lifecycle';
import type { MutationReceipt } from '../shared/types/generated/application/MutationReceipt';
import type { QueryItemsInput } from '../shared/types/generated/application/QueryItemsInput';
import type { ReorderCollectionInput } from '../shared/types/generated/application/ReorderCollectionInput';
import type { SelectionSummary as ReplacementSelectionSummary } from '../shared/types/generated/application/SelectionSummary';
import type { MediaEntityPatch } from '../shared/types/canonical';

export function queryItems(query: ItemQuery, page: ItemPageRequest): Promise<ItemPage> {
  const input: QueryItemsInput = { query, page };
  return invoke<ItemPage>('items.query', input);
}

export function recordMediaView(itemId: number): Promise<unknown> {
  return invoke('items.record_view', { item_id: itemId });
}

export function clearRecentViews(): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.clear_recent_views', {});
}

export function renameItem(itemId: number, name: string): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.rename', { item_id: itemId, name });
}

export function renameItems(renames: Array<{ item_id: number; name: string }>): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.rename_many', { renames });
}

export function patchMediaEntities(target: ItemTarget, patch: MediaEntityPatch): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.patch_metadata', { target, patch });
}

export function applyEntityTags(
  target: ItemTarget,
  operation: 'add' | 'remove',
  tags: string[],
  provenanceMask = 1,
): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.apply_tags', {
    target,
    tags,
    add: operation === 'add',
    provenance_mask: provenanceMask,
  });
}

export function setItemLifecycle(target: ItemTarget, lifecycle: Lifecycle): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.set_lifecycle', { target, lifecycle });
}

export function deleteItems(target: ItemTarget): Promise<unknown> {
  return invoke('items.delete', { target });
}

export function getSelectionSummary(target: ItemTarget): Promise<ReplacementSelectionSummary> {
  return invoke<ReplacementSelectionSummary>('items.selection_summary', { target });
}

export function organizeIntoGroup(input: OrganizeIntoCollectionInput): Promise<OrganizeIntoCollectionResult> {
  return invoke<OrganizeIntoCollectionResult>('items.organize_into_collection', input);
}

export function detachItems(input: DetachItemsInput): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.detach', input);
}

export function ungroup(itemId: number): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.ungroup', { item_id: itemId });
}

export function reorderGroup(input: ReorderCollectionInput): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.reorder_collection', input);
}
