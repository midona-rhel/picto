import { invoke } from '../platform/ipc';
import type { ItemDetails } from '../shared/types/generated/application/ItemDetails';

interface PrefetchedItemDetails {
  promise: Promise<ItemDetails>;
  value: ItemDetails | null;
  expiresAt: ReturnType<typeof setTimeout> | null;
}

const prefetchedItemDetails = new Map<number, PrefetchedItemDetails>();

function clearPrefetchedItemDetails(itemId: number, entry: PrefetchedItemDetails) {
  if (prefetchedItemDetails.get(itemId) !== entry) return;
  if (entry.expiresAt) clearTimeout(entry.expiresAt);
  prefetchedItemDetails.delete(itemId);
}

export const viewerController = {
  getItemDetails(itemId: number): Promise<ItemDetails> {
    return invoke<ItemDetails>('items.details', { item_id: itemId });
  },

  prefetchItemDetails(itemId: number): Promise<ItemDetails> {
    const existing = prefetchedItemDetails.get(itemId);
    if (existing) return existing.promise;

    let entry: PrefetchedItemDetails;
    const promise = invoke<ItemDetails>('items.details', { item_id: itemId })
      .then((details) => {
        entry.value = details;
        entry.expiresAt = setTimeout(() => clearPrefetchedItemDetails(itemId, entry), 5_000);
        return details;
      })
      .catch((reason) => {
        clearPrefetchedItemDetails(itemId, entry);
        throw reason;
      });
    entry = { promise, value: null, expiresAt: null };
    prefetchedItemDetails.set(itemId, entry);
    return entry.promise;
  },

  takePrefetchedItemDetails(itemId: number): ItemDetails | null {
    const entry = prefetchedItemDetails.get(itemId);
    if (!entry?.value) return null;
    // React Strict Mode evaluates state initializers twice before committing.
    // Defer consumption so both render passes receive the same ready payload.
    queueMicrotask(() => clearPrefetchedItemDetails(itemId, entry));
    return entry.value;
  },

  recordMediaView(itemId: number): Promise<unknown> {
    return invoke('items.record_view', { item_id: itemId });
  },
};
