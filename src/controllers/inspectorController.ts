/**
 * Inspector controller — handles async entity data loading.
 *
 * Scope transitions are committed by GridScreen directly (atomic with snapshot).
 * This controller handles:
 *   1. Entity selection: fetching details + preloading images
 */

import { getDefaultStore } from 'jotai';
import { getEntityDetails } from '../platform/entityApi';
import {
  displayedInspectorEntityDataAtom,
  displayedInspectorTargetAtom,
  inspectorErrorAtom,
  inspectorLoadingAtom,
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
