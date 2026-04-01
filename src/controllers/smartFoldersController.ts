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

  async create(name: string, icon?: string | null, color?: string | null, parentId?: number | null) {
    await api.createSmartFolder({ name, icon, color, parent_id: parentId });
    // Sidebar will refresh via state_changed event
  },

  // TODO: update — blocked on backend partial update support.
  async update(id: number, name: string, icon?: string | null, color?: string | null) {
    // Partial update not supported. Needs full struct. For now, update via the legacy path.
    // TODO: add partial update command to backend
  },
};
