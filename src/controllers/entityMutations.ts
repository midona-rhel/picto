/**
 * Entity mutation helpers — thin wrappers that build EntityTarget from a hash
 * and call the canonical API. After mutation, re-fetches inspector data.
 *
 * No optimistic updates — wait for backend confirmation + re-fetch.
 */

import * as api from '../platform/api';
import { loadInspectorData } from './inspectorController';
import type { EntityTarget } from '../shared/types/canonical';

function singleTarget(entityHash: string): EntityTarget {
  return { kind: 'entity_hashes', entity_hashes: [entityHash] };
}

export async function setEntityRating(entityHash: string, rating: number): Promise<void> {
  await api.patchMediaEntities(singleTarget(entityHash), { rating });
  void loadInspectorData(entityHash);
}

export async function setEntityName(entityHash: string, name: string): Promise<void> {
  await api.patchMediaEntities(singleTarget(entityHash), { name });
  void loadInspectorData(entityHash);
}

export async function setEntityNotes(entityHash: string, notes: string): Promise<void> {
  await api.patchMediaEntities(singleTarget(entityHash), { notes: notes.trim() || null });
  void loadInspectorData(entityHash);
}

export async function addEntityTags(entityHash: string, tags: string[]): Promise<void> {
  if (tags.length === 0) return;
  await api.applyEntityTags(singleTarget(entityHash), 'add', tags);
  void loadInspectorData(entityHash);
}

export async function removeEntityTags(entityHash: string, tags: string[]): Promise<void> {
  if (tags.length === 0) return;
  await api.applyEntityTags(singleTarget(entityHash), 'remove', tags);
  void loadInspectorData(entityHash);
}
