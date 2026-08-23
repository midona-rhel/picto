import { invoke } from './ipc';
import type { ItemPage } from '../shared/types/generated/application/ItemPage';
import type { ItemPageRequest } from '../shared/types/generated/application/ItemPageRequest';
import type { ItemQuery } from '../shared/types/generated/application/ItemQuery';
import type { ItemTarget } from '../shared/types/generated/application/ItemTarget';
import type { CollectionCoverInput } from '../shared/types/generated/application/CollectionCoverInput';
import type { DetachItemsInput } from '../shared/types/generated/application/DetachItemsInput';
import type { GroupItemsInput } from '../shared/types/generated/application/GroupItemsInput';
import type { GroupItemsResult } from '../shared/types/generated/application/GroupItemsResult';
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

export function renameItem(itemId: number, name: string): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.rename', { item_id: itemId, name });
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

export function groupItems(input: GroupItemsInput): Promise<GroupItemsResult> {
  return invoke<GroupItemsResult>('items.group', input);
}

export function detachItems(input: DetachItemsInput): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.detach', input);
}

export function ungroupCollection(itemId: number): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.ungroup', { item_id: itemId });
}

export function reorderCollection(input: ReorderCollectionInput): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.reorder_collection', input);
}

export function setCollectionCover(input: CollectionCoverInput): Promise<MutationReceipt> {
  return invoke<MutationReceipt>('items.set_collection_cover', input);
}
