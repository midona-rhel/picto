/**
 * Smart folder controller — owns smart folder actions.
 * Calls API, then eagerly updates sidebar state atoms.
 *
 * NOTE: Smart folder rename/color/icon require sending the full SmartFolder
 * struct to the backend (update_smart_folder). The backend has no partial
 * patch command. These actions are disabled until either:
 *   - a partial update command is added to the backend, or
 *   - the smart folder edit modal (which has the full data) is built
 */

import { getDefaultStore } from 'jotai';
import * as api from '../platform/api';
import { removeSmartFolderNodeAtom } from '../state/sidebar';

const store = getDefaultStore();

export const smartFoldersController = {
  async delete(id: string) {
    const numId = parseInt(id, 10);
    if (!isNaN(numId)) store.set(removeSmartFolderNodeAtom, numId);
    await api.deleteSmartFolder(id);
  },

  // TODO: rename, applyColor, applyIcon — blocked on backend partial update support.
  // The update_smart_folder command requires a full SmartFolder struct
  // (smart_folder_id, name, parent_id, icon, color, predicate_json, sort_field, sort_order).
  // Sending only { name } will fail deserialization.
  //
  // Options to unblock:
  //   1. Add a partial update command to the backend (preferred)
  //   2. Read the full smart folder from sidebar node meta, patch, then send

  // TODO: create(name, parentId, predicate) — needs smart folder create modal
};
