import { invoke } from './ipc';
import type { ItemPage } from '../shared/types/generated/application/ItemPage';
import type { ItemPageRequest } from '../shared/types/generated/application/ItemPageRequest';
import type { ItemQuery } from '../shared/types/generated/application/ItemQuery';
import type { ItemTarget } from '../shared/types/generated/application/ItemTarget';
import type { Lifecycle } from '../shared/types/generated/application/Lifecycle';
import type { MutationReceipt } from '../shared/types/generated/application/MutationReceipt';
import type { QueryItemsInput } from '../shared/types/generated/application/QueryItemsInput';
import type { SelectionSummary as ReplacementSelectionSummary } from '../shared/types/generated/application/SelectionSummary';
import type {
  CanonicalEntityDetails,
  MediaEntityPatch,
} from '../shared/types/canonical';

export function queryItems(query: ItemQuery, page: ItemPageRequest): Promise<ItemPage> {
  const input: QueryItemsInput = { query, page };
  return invoke<ItemPage>('items.query', input);
}

export function getEntityDetails(entityHash: string): Promise<CanonicalEntityDetails | null> {
  return invoke<CanonicalEntityDetails | null>('get_entity_details', { entity_hash: entityHash });
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
