/**
 * Entity mutation helpers — thin wrappers that build EntityTarget from a hash
 * and call the canonical API. After mutation, re-fetches inspector data.
 *
 * No optimistic updates — wait for backend confirmation + re-fetch.
 */

import {
  applyEntityTags,
  deleteItems,
  getSelectionSummary,
  patchMediaEntities,
  renameItem,
  setItemLifecycle,
} from '../platform/entityApi';
import { removeEntitiesFromFolder, updateFolderMembership } from '../platform/folderApi';
import { loadInspectorData } from './inspectorController';
import { recordRecentItems } from '../shared/hooks/useRecentItems';
import type { ItemTarget } from '../shared/types/generated/application/ItemTarget';
import type { Lifecycle } from '../shared/types/generated/application/Lifecycle';
import type { SelectionSummary } from '../shared/types/generated/application/SelectionSummary';

function singleTarget(itemId: number): ItemTarget {
  return { kind: 'explicit', item_ids: [itemId] };
}

function maybeReloadSingleItem(target: ItemTarget): void {
  const itemIds = target.kind === 'explicit' ? target.item_ids : [];
  if (itemIds.length === 1) {
    void loadInspectorData(itemIds[0]);
  }
}

export async function setTargetRating(target: ItemTarget, rating: number): Promise<void> {
  await patchMediaEntities(target, { rating });
  maybeReloadSingleItem(target);
}

export async function setItemName(itemId: number, name: string): Promise<void> {
  await renameItem(itemId, name);
  void loadInspectorData(itemId);
}

export async function setTargetNotes(target: ItemTarget, notes: string): Promise<void> {
  await patchMediaEntities(target, { notes: notes.trim() || null });
  maybeReloadSingleItem(target);
}

export async function addTargetTags(target: ItemTarget, tags: string[]): Promise<void> {
  if (tags.length === 0) return;
  await applyEntityTags(target, 'add', tags);
  recordRecentItems('picto-recent-tags', tags);
  maybeReloadSingleItem(target);
}

export async function removeTargetTags(target: ItemTarget, tags: string[]): Promise<void> {
  if (tags.length === 0) return;
  await applyEntityTags(target, 'remove', tags);
  maybeReloadSingleItem(target);
}

export async function setTargetLifecycle(target: ItemTarget, lifecycle: Lifecycle): Promise<void> {
  await setItemLifecycle(target, lifecycle);
  maybeReloadSingleItem(target);
}

export async function permanentlyDeleteTarget(target: ItemTarget): Promise<void> {
  await deleteItems(target);
}

export async function updateTargetFolderMembership(
  target: ItemTarget,
  folderId: number,
  operation: 'add' | 'remove',
): Promise<void> {
  await updateFolderMembership(target, folderId, operation);
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
  await patchMediaEntities(singleTarget(itemId), { source_urls: urls });
  void loadInspectorData(itemId);
}
