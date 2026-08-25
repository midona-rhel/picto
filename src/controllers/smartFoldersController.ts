/** Smart-folder CRUD through the replacement transaction boundary. */

import { getDefaultStore } from 'jotai';
import {
  createSmartFolder,
  deleteSmartFolder,
  moveSmartFolder,
  reorderSmartFolders,
  updateSmartFolder,
} from '../platform/smartFolderApi';
import type {
  SmartFolderCommandPayload,
  SmartFolderPredicate as UiSmartFolderPredicate,
} from '../shared/types/canonical';
import type { CreateSmartFolderInput } from '../shared/types/generated/application/CreateSmartFolderInput';
import type { SmartFolderPredicate } from '../shared/types/generated/application/SmartFolderPredicate';
import { activeNodeIdAtom } from '../state/navigation';
import { navigateToNode, removeHistoryEntries } from '../state/navigationHistory';
import { announceUndoableMutation } from '../runtime/historyRuntime';
import { gridController } from './gridController';
import { sidebarController } from './sidebarController';

const store = getDefaultStore();

function toInput(folder: SmartFolderCommandPayload): CreateSmartFolderInput {
  const parsed = JSON.parse(folder.predicate_json) as UiSmartFolderPredicate;
  const predicate: SmartFolderPredicate = {
    groups: parsed.groups.map((group) => ({
      match_mode: group.match_mode,
      negate: group.negate ?? false,
      rules: group.rules.map((rule) => ({
        field: rule.field,
        op: rule.op,
        value: rule.value ?? null,
        value2: rule.value2 ?? null,
        values: rule.values ?? null,
      })),
    })),
  };
  return {
    name: folder.name,
    parent_id: folder.parent_id,
    predicate,
    icon: folder.icon,
    color: folder.color,
    notes: folder.notes,
    sort_field: folder.sort_field ?? null,
    sort_order: folder.sort_order ?? null,
  };
}

export function emptySmartFolderPayload(
  overrides: Partial<SmartFolderCommandPayload> = {},
): SmartFolderCommandPayload {
  const predicate: UiSmartFolderPredicate = { groups: [] };
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
  createGroup(name = 'Untitled', parentId: number | null = null): Promise<string> {
    return this.create(emptySmartFolderPayload({ name, parent_id: parentId }));
  },

  async refresh(id: number): Promise<void> {
    await sidebarController.fetchTree();
    if (store.get(activeNodeIdAtom) === `smart:${id}`) {
      await gridController.loadFirstPage({ preserveItems: true });
    }
  },

  async delete(id: string): Promise<void> {
    const smartFolderId = Number(id);
    if (!Number.isSafeInteger(smartFolderId)) throw new Error(`Invalid smart folder ID: ${id}`);
    const result = await deleteSmartFolder(smartFolderId);
    const deleted = new Set(result.deleted_smart_folder_ids.map((value) => `smart:${value}`));
    removeHistoryEntries(deleted);
    if (deleted.has(store.get(activeNodeIdAtom))) {
      const fallback = result.fallback_smart_folder_id == null
        ? 'system:active'
        : `smart:${result.fallback_smart_folder_id}`;
      navigateToNode(fallback);
    }
    await announceUndoableMutation('smart_folders.delete');
  },

  async create(folder: SmartFolderCommandPayload): Promise<string> {
    const result = await createSmartFolder(toInput(folder));
    await announceUndoableMutation('smart_folders.create');
    return `smart:${result.smart_folder_id}`;
  },

  async update(id: number, folder: SmartFolderCommandPayload): Promise<void> {
    await updateSmartFolder(id, toInput(folder));
    await announceUndoableMutation('smart_folders.update');
  },

  async move(smartFolderId: number, parentId: number | null, siblingOrder: [number, number][]) {
    await moveSmartFolder(smartFolderId, parentId);
    if (siblingOrder.length > 0) {
      const orderedIds = [...siblingOrder]
        .sort((left, right) => left[1] - right[1])
        .map(([siblingId]) => siblingId);
      await reorderSmartFolders(parentId, orderedIds);
    }
    await announceUndoableMutation(
      siblingOrder.length > 0 ? 'smart_folders.reorder' : 'smart_folders.move',
    );
  },
};
