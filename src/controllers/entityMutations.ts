/**
 * Entity mutation helpers — thin wrappers that build EntityTarget from a hash
 * and call the canonical API. The inspector re-reads itself from the
 * `library/changed` event the mutation publishes (see inspectorSettle), so these
 * helpers never reload it directly; doing both loaded every edit twice.
 *
 * No optimistic updates — wait for backend confirmation + re-fetch.
 */

import { getDefaultStore } from 'jotai';
import {
  applyEntityTags,
  deleteItems,
  rejectDeleteItems,
  getSelectionSummary,
  getCollectionNoteDraft,
  patchMediaEntities,
  renameItem,
  renameItems,
  setItemLifecycle,
} from '../platform/entityApi';
import { removeEntitiesFromFolder, updateFolderMembership } from '../platform/folderApi';
import { getSettings } from '../platform/settingsApi';
import { recordRecentItems } from '../shared/hooks/useRecentItems';
import { recordRecentFolderUse } from '../shared/hooks/useRecentFolders';
import type { CollectionNoteDraft, EntityTarget, Lifecycle, Rating, SelectionSummary } from '../shared/types/canonical';
import { clearSelectionAtom } from '../state/selection';
import { permanentDeletesInFlightAtom } from '../state/mutationActivity';
import { quickLookSessionAtom, viewerSessionAtom } from '../state/viewer';
import { announceUndoableMutation } from '../runtime/historyRuntime';
import { getHistoryState } from '../platform/historyApi';
import { requestPermanentDelete } from './permanentDeleteRequest';

const store = getDefaultStore();

function singleTarget(itemId: number): EntityTarget {
  return { kind: 'explicit', root_ids: [itemId] };
}

/** A successful mutation removed or relocated the grid selection as a unit. */
export function settleSelectionAfterMutation(): void {
  store.set(clearSelectionAtom);
}

export async function setTargetRating(target: EntityTarget, rating: number): Promise<void> {
  await patchMediaEntities(target, { rating: ratingName(rating) });
  await announceUndoableMutation('items.patch_metadata');
}

export async function setItemName(itemId: number, name: string): Promise<void> {
  await renameItem(itemId, name);
  await announceUndoableMutation('items.rename');
}

export async function setItemNames(renames: Array<{ root_id: number; name: string }>): Promise<void> {
  await renameItems(renames);
  await announceUndoableMutation('items.rename_many');
}

export async function setTargetNotes(target: EntityTarget, notes: string): Promise<void> {
  await patchMediaEntities(target, { notes: notes.trim() || null });
  await announceUndoableMutation('items.patch_metadata');
}

export async function setTargetSourceUrls(target: EntityTarget, urls: string[]): Promise<void> {
  await patchMediaEntities(target, { source_urls: urls });
  await announceUndoableMutation('items.patch_metadata');
}

export async function addTargetTags(target: EntityTarget, tags: string[]): Promise<void> {
  if (tags.length === 0) return;
  await applyEntityTags(target, 'add', tags);
  await announceUndoableMutation('items.apply_tags');
  recordRecentItems('picto-recent-tags', tags);
}

export async function removeTargetTags(target: EntityTarget, tags: string[]): Promise<void> {
  if (tags.length === 0) return;
  await applyEntityTags(target, 'remove', tags);
  await announceUndoableMutation('items.apply_tags');
}

export async function setTargetLifecycle(target: EntityTarget, lifecycle: Lifecycle): Promise<void> {
  await setItemLifecycle(target, lifecycle);
  await announceUndoableMutation('items.set_lifecycle');
  settleSelectionAfterMutation();
}

export async function permanentlyDeleteTarget(target: EntityTarget, requireUndoable = true): Promise<void> {
  await deleteTarget(target, deleteItems, 'items.delete', requireUndoable);
}

/**
 * Reject Inbox items: to Trash, or straight out of the library when the
 * "Rejected items" setting says so. Confirm deletes that cannot be undone.
 */
export async function rejectTarget(target: EntityTarget): Promise<boolean> {
  const action = await getSettings().then((settings) => settings.rejectAction).catch(() => 'trash' as const);
  if (action === 'delete') {
    return requestPermanentDelete(target, null, {
      onlyNonUndoable: true,
      remove: (selection, requireUndoable) => deleteTarget(selection, rejectDeleteItems, 'items.reject', requireUndoable),
    });
  }
  await setTargetLifecycle(target, 'trash');
  return true;
}

async function deleteTarget(
  target: EntityTarget,
  remove: (target: EntityTarget, requireUndoable: boolean) => Promise<unknown>,
  command: string,
  requireUndoable: boolean,
): Promise<void> {
  store.set(permanentDeletesInFlightAtom, (count) => count + 1);
  try {
    const before = await getHistoryState().catch(() => undefined);
    await remove(target, requireUndoable);
    if (before) await announceUndoableMutation(command, before.undo?.entry_id ?? null);

    if (target.kind === 'explicit') {
      const deletedIds = new Set(target.root_ids);
      const viewer = store.get(viewerSessionAtom);
      const quickLook = store.get(quickLookSessionAtom);
      if (viewer && deletedIds.has(viewer.currentItemId)) store.set(viewerSessionAtom, null);
      if (quickLook && deletedIds.has(quickLook.currentItemId)) store.set(quickLookSessionAtom, null);
    }
    settleSelectionAfterMutation();
  } finally {
    store.set(permanentDeletesInFlightAtom, (count) => Math.max(0, count - 1));
  }
}

export async function updateTargetFolderMembership(
  target: EntityTarget,
  folderId: number,
  operation: 'add' | 'remove',
): Promise<void> {
  await updateFolderMembership(target, folderId, operation);
  await announceUndoableMutation('items.set_folder');
  if (operation === 'add') recordRecentFolderUse([folderId]);
}

export async function getTargetSelectionSummary(target: EntityTarget): Promise<SelectionSummary> {
  return getSelectionSummary(target);
}

export async function getTargetCollectionNoteDraft(target: EntityTarget): Promise<CollectionNoteDraft> {
  return getCollectionNoteDraft(target);
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
}

export async function setItemSourceUrls(itemId: number, urls: string[]): Promise<void> {
  await setTargetSourceUrls(singleTarget(itemId), urls);
}

function ratingName(value: number): Rating {
  return (['unrated', 'one', 'two', 'three', 'four', 'five'][value] ?? 'unrated') as Rating;
}
