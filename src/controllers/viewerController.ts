import { invoke } from '../platform/ipc';
import type { ItemDetails } from '../shared/types/generated/application/ItemDetails';

export const viewerController = {
  getItemDetails(itemId: number): Promise<ItemDetails> {
    return invoke<ItemDetails>('items.details', { item_id: itemId });
  },

  recordMediaView(itemId: number): Promise<unknown> {
    return invoke('items.record_view', { item_id: itemId });
  },
};
