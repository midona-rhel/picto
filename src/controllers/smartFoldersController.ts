/**
 * Smart folder controller — owns smart folder actions.
 * Calls API, then eagerly updates sidebar state atoms.
 */

import { getDefaultStore } from 'jotai';
import {
  createSmartFolder,
  deleteSmartFolder,
  updateSmartFolder,
} from '../platform/smartFolderApi';
import type { SmartFolderCommandPayload, SmartFolderPredicate } from '../shared/types/canonical';
import { removeSmartFolderNodeAtom } from '../state/sidebar';

const store = getDefaultStore();

export function emptySmartFolderPayload(overrides: Partial<SmartFolderCommandPayload> = {}): SmartFolderCommandPayload {
  const predicate: SmartFolderPredicate = { groups: [] };
  return {
    smart_folder_id: overrides.smart_folder_id ?? 0,
    name: overrides.name ?? 'New Smart Folder',
    parent_id: overrides.parent_id ?? null,
    icon: overrides.icon ?? null,
    color: overrides.color ?? null,
    notes: overrides.notes ?? null,
    predicate_json: overrides.predicate_json ?? JSON.stringify(predicate),
    sort_field: overrides.sort_field ?? null,
    sort_order: overrides.sort_order ?? null,
    display_order: overrides.display_order ?? null,
    created_at: overrides.created_at ?? null,
    updated_at: overrides.updated_at ?? null,
  };
}

export const smartFoldersController = {
  async delete(id: string) {
    const numId = parseInt(id, 10);
    if (!isNaN(numId)) store.set(removeSmartFolderNodeAtom, numId);
    await deleteSmartFolder(id);
  },

  async create(folder: SmartFolderCommandPayload) {
    await createSmartFolder({ folder });
    // Sidebar will refresh via state_changed event
  },

  async update(id: number, folder: SmartFolderCommandPayload) {
    await updateSmartFolder({ id: String(id), folder });
  },
};
