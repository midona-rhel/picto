/**
 * Entity mutation helpers — thin wrappers that build EntityTarget from a hash
 * and call the canonical API. After mutation, re-fetches inspector data.
 *
 * No optimistic updates — wait for backend confirmation + re-fetch.
 */

import { getDefaultStore } from 'jotai';
import {
  applyEntityTags,
  deleteItems,
  getSelectionSummary,
  patchMediaEntities,
  renameItem,
  renameItems,
  setItemLifecycle,
} from '../platform/entityApi';
import { removeEntitiesFromFolder, updateFolderMembership } from '../platform/folderApi';
import { loadInspectorData } from './inspectorController';
import { recordRecentItems } from '../shared/hooks/useRecentItems';
import type { ItemTarget } from '../shared/types/generated/application/ItemTarget';
import type { Lifecycle } from '../shared/types/generated/application/Lifecycle';
import type { SelectionSummary } from '../shared/types/generated/application/SelectionSummary';
import { clearSelectionAtom } from '../state/selection';
import { announceUndoableMutation } from '../runtime/historyRuntime';

const store = getDefaultStore();

function singleTarget(itemId: number): ItemTarget {
  return { kind: 'explicit', item_ids: [itemId] };
}

function maybeReloadSingleItem(target: ItemTarget): void {
  const itemIds = target.kind === 'explicit' ? target.item_ids : [];
  if (itemIds.length === 1) {
    void loadInspectorData(itemIds[0]);
  }
}

/** A successful mutation removed or relocated the grid selection as a unit. */
export function settleSelectionAfterMutation(): void {
  store.set(clearSelectionAtom);
}

export async function setTargetRating(target: ItemTarget, rating: number): Promise<void> {
  await patchMediaEntities(target, { rating });
  await announceUndoableMutation('items.patch_metadata');
  maybeReloadSingleItem(target);
}

export async function setItemName(itemId: number, name: string): Promise<void> {
  await renameItem(itemId, name);
  await announceUndoableMutation('items.rename');
  void loadInspectorData(itemId);
}

export async function setItemNames(renames: Array<{ item_id: number; name: string }>): Promise<void> {
  await renameItems(renames);
  await announceUndoableMutation('items.rename_many');
}

export async function setTargetNotes(target: ItemTarget, notes: string): Promise<void> {
  await patchMediaEntities(target, { notes: notes.trim() || null });
  await announceUndoableMutation('items.patch_metadata');
  maybeReloadSingleItem(target);
}

export async function setTargetSourceUrls(target: ItemTarget, urls: string[]): Promise<void> {
  await patchMediaEntities(target, { source_urls: urls });
  await announceUndoableMutation('items.patch_metadata');
  maybeReloadSingleItem(target);
}

export async function addTargetTags(target: ItemTarget, tags: string[]): Promise<void> {
  if (tags.length === 0) return;
  await applyEntityTags(target, 'add', tags);
  await announceUndoableMutation('items.apply_tags');
  recordRecentItems('picto-recent-tags', tags);
  maybeReloadSingleItem(target);
}

export async function removeTargetTags(target: ItemTarget, tags: string[]): Promise<void> {
  if (tags.length === 0) return;
  await applyEntityTags(target, 'remove', tags);
  await announceUndoableMutation('items.apply_tags');
  maybeReloadSingleItem(target);
}

export async function setTargetLifecycle(target: ItemTarget, lifecycle: Lifecycle): Promise<void> {
  await setItemLifecycle(target, lifecycle);
  await announceUndoableMutation('items.set_lifecycle');
  settleSelectionAfterMutation();
}

export async function permanentlyDeleteTarget(target: ItemTarget): Promise<void> {
  await deleteItems(target);
  settleSelectionAfterMutation();
}

export async function updateTargetFolderMembership(
  target: ItemTarget,
  folderId: number,
  operation: 'add' | 'remove',
): Promise<void> {
  await updateFolderMembership(target, folderId, operation);
  await announceUndoableMutation('items.set_folder');
  if (operation === 'add') recordRecentItems('picto-recent-folders', [String(folderId)]);
  maybeReloadSingleItem(target);
}

export async function getTargetSelectionSummary(target: ItemTarget): Promise<SelectionSummary> {
  return getSelectionSummary(target);
}

export async function setItemRating(itemId: number, rating: number): Promise<void> {
  await setTargetRating(singleTarget(itemId), rating);
}

export async function setItemNotes(itemId: number, notes: string): Promise<void> {
  await setTargetNotes(singleTarget(itemId), notes);
}

export async function removeItemTags(itemId: number, tags: string[]): Promise<void> {
  await removeTargetTags(singleTarget(itemId), tags);
}

export async function removeItemFromFolder(itemId: number, folderId: number): Promise<void> {
  await removeEntitiesFromFolder(folderId, singleTarget(itemId));
  void loadInspectorData(itemId);
}

export async function setItemSourceUrls(itemId: number, urls: string[]): Promise<void> {
  await setTargetSourceUrls(singleTarget(itemId), urls);
}
