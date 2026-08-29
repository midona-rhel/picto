/**
 * Inspector controller — handles async entity data loading.
 *
 * Scope transitions are committed by GridScreen directly (atomic with snapshot).
 * This controller handles:
 *   1. Entity selection: fetching details + preloading images
 */

import { getDefaultStore } from 'jotai';
import { invoke } from '../platform/ipc';
import type { CanonicalEntityDetails } from '../shared/types/canonical';
import { tagsController } from './tagsController';
import {
  displayedInspectorItemDetailsAtom,
  displayedInspectorTargetAtom,
  inspectorErrorAtom,
  inspectorLoadingAtom,
} from '../state/inspector';

const store = getDefaultStore();

let loadVersion = 0;
let loadingTimer: ReturnType<typeof setTimeout> | null = null;

export function cancelInspectorLoad() {
  loadVersion += 1;
  if (loadingTimer) clearTimeout(loadingTimer);
  loadingTimer = null;
}

function preloadImage(src: string): Promise<void> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => resolve();
    img.onerror = () => resolve();
    img.src = src;
  });
}

export async function loadInspectorData(itemId: number | null) {
  if (itemId == null) {
    cancelInspectorLoad();
    return;
  }

  const v = ++loadVersion;
  if (loadingTimer) clearTimeout(loadingTimer);
  loadingTimer = setTimeout(() => {
    if (v !== loadVersion) return;
    store.set(displayedInspectorTargetAtom, { kind: 'item', itemId });
    store.set(displayedInspectorItemDetailsAtom, null);
    store.set(inspectorLoadingAtom, true);
    store.set(inspectorErrorAtom, null);
  }, 250);

  try {
    const result = await invoke<CanonicalEntityDetails>('items.details', { root_id: itemId });
    if (v !== loadVersion || !result) return;
    const displayHash = result.media[0]?.facts.content_hash;
    const [resolvedTags] = await Promise.all([
      result.tag_ids.length > 0
        ? tagsController.getById(result.tag_ids).catch(() => [])
        : Promise.resolve([]),
      displayHash
        ? preloadImage(`media://localhost/thumb/${displayHash}.jpg`)
        : Promise.resolve(),
    ]);
    if (v !== loadVersion) return;
    if (loadingTimer) clearTimeout(loadingTimer);
    loadingTimer = null;
    store.set(displayedInspectorItemDetailsAtom, { ...result, resolved_tag_records: resolvedTags });
    store.set(displayedInspectorTargetAtom, { kind: 'item', itemId: result.root.root_id });
    store.set(inspectorLoadingAtom, false);
    store.set(inspectorErrorAtom, null);
  } catch (err) {
    if (v !== loadVersion) return;
    if (loadingTimer) clearTimeout(loadingTimer);
    loadingTimer = null;
    store.set(displayedInspectorTargetAtom, { kind: 'item', itemId });
    store.set(displayedInspectorItemDetailsAtom, null);
    store.set(inspectorErrorAtom, err instanceof Error ? err.message : String(err));
    store.set(inspectorLoadingAtom, false);
  }
}
