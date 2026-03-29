/**
 * Inspector controller — handles async entity data loading.
 *
 * Scope transitions are committed by GridScreen directly (atomic with snapshot).
 * This controller only handles entity selection: fetching details + preloading images.
 */

import { getDefaultStore } from 'jotai';
import * as api from '../platform/api';
import {
  displayedInspectorEntityDataAtom,
  displayedInspectorTargetAtom,
  inspectorErrorAtom,
  inspectorLoadingAtom,
  liveInspectorTargetAtom,
} from '../state/inspector';

const store = getDefaultStore();

let loadVersion = 0;
let subscribed = false;

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
  store.set(inspectorLoadingAtom, true);
  store.set(inspectorErrorAtom, null);

  try {
    const result = await api.getEntityDetails(entityHash);
    if (v !== loadVersion || !result) return;
    await preloadImage(`media://localhost/thumb/${result.thumbnail_hash ?? result.entity_hash}.jpg`);
    if (v !== loadVersion) return;
    store.set(displayedInspectorEntityDataAtom, result);
    store.set(displayedInspectorTargetAtom, { kind: 'entity', entityHash: result.entity_hash });
  } catch (err) {
    if (v !== loadVersion) return;
    store.set(inspectorErrorAtom, err instanceof Error ? err.message : String(err));
  } finally {
    if (v === loadVersion) store.set(inspectorLoadingAtom, false);
  }
}

/** Start watching live selection for entity data loading. */
export function startInspectorSync() {
  if (subscribed) return;
  subscribed = true;

  let lastEntityHash = '';
  store.sub(liveInspectorTargetAtom, () => {
    const target = store.get(liveInspectorTargetAtom);
    if (target.kind === 'entity') {
      if (target.entityHash !== lastEntityHash) {
        lastEntityHash = target.entityHash;
        void loadInspectorData(target.entityHash);
      }
    } else if (target.kind === 'multi') {
      lastEntityHash = '';
      loadVersion++;
      store.set(displayedInspectorEntityDataAtom, null);
      store.set(displayedInspectorTargetAtom, target);
      store.set(inspectorLoadingAtom, false);
      store.set(inspectorErrorAtom, null);
    } else {
      lastEntityHash = '';
      loadVersion++;
      // Commit scope/none target immediately so the inspector switches
      // back from entity/multi view when selection is cleared.
      store.set(displayedInspectorEntityDataAtom, null);
      store.set(displayedInspectorTargetAtom, target);
      store.set(inspectorLoadingAtom, false);
      store.set(inspectorErrorAtom, null);
    }
  });
}
