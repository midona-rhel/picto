/**
 * Entity mutation helpers — thin wrappers that build EntityTarget from a hash
 * and call the canonical API. After mutation, re-fetches inspector data.
 *
 * No optimistic updates — wait for backend confirmation + re-fetch.
 */

import {
  applyEntityTags,
  deleteEntities,
  getSelectionSummary,
  patchMediaEntities,
  setEntityStatus,
} from '../platform/entityApi';
import { removeEntitiesFromFolder, updateFolderMembership } from '../platform/folderApi';
import { loadInspectorData } from './inspectorController';
import { recordRecentItems } from '../shared/hooks/useRecentItems';
import type { EntityTarget, SelectionSummary } from '../shared/types/canonical';

function singleTarget(entityHash: string): EntityTarget {
  return { kind: 'entity_hashes', entity_hashes: [entityHash] };
}

function maybeReloadSingleEntity(target: EntityTarget): void {
  const hashes = target.kind === 'entity_hashes' ? target.entity_hashes ?? [] : [];
  if (hashes.length === 1) {
    void loadInspectorData(hashes[0]);
  }
}

export async function setTargetRating(target: EntityTarget, rating: number): Promise<void> {
  await patchMediaEntities(target, { rating });
  maybeReloadSingleEntity(target);
}

export async function setTargetName(target: EntityTarget, name: string): Promise<void> {
  await patchMediaEntities(target, { name });
  maybeReloadSingleEntity(target);
}

export async function setTargetNotes(target: EntityTarget, notes: string): Promise<void> {
  await patchMediaEntities(target, { notes: notes.trim() || null });
  maybeReloadSingleEntity(target);
}

export async function addTargetTags(target: EntityTarget, tags: string[]): Promise<void> {
  if (tags.length === 0) return;
  await applyEntityTags(target, 'add', tags);
  recordRecentItems('picto-recent-tags', tags);
  maybeReloadSingleEntity(target);
}

export async function removeTargetTags(target: EntityTarget, tags: string[]): Promise<void> {
  if (tags.length === 0) return;
  await applyEntityTags(target, 'remove', tags);
  maybeReloadSingleEntity(target);
}

export async function setTargetStatus(target: EntityTarget, status: number): Promise<void> {
  await setEntityStatus(target, status);
  maybeReloadSingleEntity(target);
}

export async function permanentlyDeleteTarget(target: EntityTarget): Promise<void> {
  await deleteEntities(target);
}

export async function updateTargetFolderMembership(
  target: EntityTarget,
  folderId: number,
  operation: 'add' | 'remove',
): Promise<void> {
  await updateFolderMembership(target, folderId, operation);
  if (operation === 'add') recordRecentItems('picto-recent-folders', [String(folderId)]);
  maybeReloadSingleEntity(target);
}

export async function getTargetSelectionSummary(target: EntityTarget): Promise<SelectionSummary> {
  return getSelectionSummary(target);
}

export async function setEntityRating(entityHash: string, rating: number): Promise<void> {
  await setTargetRating(singleTarget(entityHash), rating);
}

export async function setEntityName(entityHash: string, name: string): Promise<void> {
  await setTargetName(singleTarget(entityHash), name);
}

export async function setEntityNotes(entityHash: string, notes: string): Promise<void> {
  await setTargetNotes(singleTarget(entityHash), notes);
}

export async function removeEntityTags(entityHash: string, tags: string[]): Promise<void> {
  await removeTargetTags(singleTarget(entityHash), tags);
}

export async function removeEntityFromFolder(entityHash: string, folderId: number): Promise<void> {
  await removeEntitiesFromFolder(folderId, singleTarget(entityHash));
  void loadInspectorData(entityHash);
}

export async function setEntitySourceUrls(entityHash: string, urls: string[]): Promise<void> {
  await patchMediaEntities(singleTarget(entityHash), { source_urls: urls.length > 0 ? urls : null });
  void loadInspectorData(entityHash);
}
