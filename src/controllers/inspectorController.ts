/**
 * Inspector controller — handles async entity data loading.
 *
 * Scope transitions are committed by GridScreen directly (atomic with snapshot).
 * This controller handles:
 *   1. Entity selection: fetching details + preloading images
 */

import { getDefaultStore } from 'jotai';
import { getEntityDetails, queryEntityView } from '../platform/entityApi';
import {
  displayedInspectorEntityDataAtom,
  displayedInspectorTargetAtom,
  inspectorErrorAtom,
  inspectorLoadingAtom,
  liveInspectorTargetAtom,
  subfolderPreviewAtom,
  type InspectorTarget,
} from '../state/inspector';

const store = getDefaultStore();

let loadVersion = 0;


function preloadImage(src: string): Promise<void> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => resolve();
    img.onerror = () => resolve();
    img.src = src;
  });
}

export async function loadInspectorData(entityHash: string | null) {
  if (!entityHash) return;

  const v = ++loadVersion;
  // Don't set loading or clear data — keep showing the current entity until new data arrives.
  // This prevents the inspector from flashing empty/partial state during navigation.

  try {
    const result = await getEntityDetails(entityHash);
    if (v !== loadVersion || !result) return;
    await preloadImage(`media://localhost/thumb/${result.entity_hash}.jpg`);
    if (v !== loadVersion) return;
    // Atomic swap — old data visible until this point
    store.set(displayedInspectorEntityDataAtom, result);
    store.set(displayedInspectorTargetAtom, { kind: 'entity', entityHash: result.entity_hash });
    store.set(inspectorLoadingAtom, false);
    store.set(inspectorErrorAtom, null);
  } catch (err) {
    if (v !== loadVersion) return;
    store.set(inspectorErrorAtom, err instanceof Error ? err.message : String(err));
    store.set(inspectorLoadingAtom, false);
  }
}

export async function loadSubfolderInspectorPreview(nodeId: string) {
  const folderId = Number.parseInt(nodeId.replace('folder:', ''), 10);
  if (!nodeId.startsWith('folder:') || Number.isNaN(folderId)) return;

  const v = ++loadVersion;
  try {
    const page = await queryEntityView({
      base_scope: { kind: 'folder', id: folderId },
      page: { limit: 4 },
    });
    const liveTarget = store.get(liveInspectorTargetAtom);
    if (v !== loadVersion || liveTarget.kind !== 'scope' || liveTarget.nodeId !== nodeId) return;

    store.set(subfolderPreviewAtom, {
      nodeId,
      items: page.items.slice(0, 4),
      totalCount: page.total_count,
      totalSizeBytes: page.total_size_bytes,
    });
    store.set(displayedInspectorEntityDataAtom, null);
    store.set(displayedInspectorTargetAtom, { kind: 'scope', nodeId });
    store.set(inspectorLoadingAtom, false);
    store.set(inspectorErrorAtom, null);
  } catch (err) {
    if (v !== loadVersion) return;
    store.set(inspectorErrorAtom, err instanceof Error ? err.message : String(err));
  }
}

/** Commit a non-entity target and invalidate any preview/entity request in flight. */
export function commitInspectorTarget(target: Exclude<InspectorTarget, { kind: 'entity' }>) {
  loadVersion++;
  store.set(subfolderPreviewAtom, null);
  store.set(displayedInspectorEntityDataAtom, null);
  store.set(displayedInspectorTargetAtom, target);
  store.set(inspectorLoadingAtom, false);
  store.set(inspectorErrorAtom, null);
}
