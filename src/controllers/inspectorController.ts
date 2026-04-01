/**
 * Inspector controller — handles async entity data loading.
 *
 * Scope transitions are committed by GridScreen directly (atomic with snapshot).
 * This controller handles:
 *   1. Entity selection: fetching details + preloading images
 *   2. State change events: re-fetching when tags/folders/status change for the displayed entity
 */

import { getDefaultStore } from 'jotai';
import * as api from '../platform/api';
import { listen } from '../platform/ipc';
import {
  displayedInspectorEntityDataAtom,
  displayedInspectorTargetAtom,
  inspectorErrorAtom,
  inspectorLoadingAtom,
  inspectorPinnedAtom,
  liveInspectorTargetAtom,
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

/** Start watching live selection for entity data loading.
 *  Returns a cleanup function (HMR safety). */
export function startInspectorSync(): () => void {
  let cancelled = false;
  let lastEntityHash = '';
  const unsubTarget = store.sub(liveInspectorTargetAtom, () => {
    if (cancelled) return;
    // When pinned, ignore all selection changes
    if (store.get(inspectorPinnedAtom)) return;

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

  // Re-fetch displayed entity when backend reports relevant changes.
  const unlistenPromise = listen<{ changes: Record<string, unknown>; seq?: number }>('runtime/state_changed', (event) => {
    if (cancelled) return;

    const changes = event.payload.changes;
    const relevant = changes.tags_changed
      || changes.status_changed
      || changes.folder_membership_changed
      || changes.media_metadata_changed;
    if (!relevant) return;

    const data = store.get(displayedInspectorEntityDataAtom);
    if (!data) return;

    const hashes = changes.entity_hashes as string[] | undefined;
    if (hashes?.length && !hashes.includes(data.entity_hash)) return;

    void loadInspectorData(data.entity_hash);
  });

  return () => {
    cancelled = true;
    unsubTarget();
    unlistenPromise.then((fn) => fn()).catch(() => {});
  };
}
