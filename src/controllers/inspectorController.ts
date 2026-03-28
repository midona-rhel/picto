/**
 * Inspector controller — commits displayed inspector state.
 *
 * Live selection/navigation state can move ahead of the displayed grid during
 * scope transitions. This controller only commits inspector content when the
 * next entity or displayed scope is actually ready to render.
 */

import { getDefaultStore } from 'jotai';
import * as api from '../platform/api';
import {
  displayedGridSnapshotAtom,
  displayedInspectorEntityDataAtom,
  displayedInspectorTargetAtom,
  inspectorErrorAtom,
  inspectorLoadingAtom,
  liveInspectorTargetAtom,
  type InspectorTarget,
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

function commitScopeFromSnapshot() {
  const snapshot = store.get(displayedGridSnapshotAtom);
  if (!snapshot) {
    store.set(displayedInspectorTargetAtom, { kind: 'none' });
    store.set(displayedInspectorEntityDataAtom, null);
    store.set(inspectorLoadingAtom, false);
    store.set(inspectorErrorAtom, null);
    return;
  }
  store.set(displayedInspectorEntityDataAtom, null);
  store.set(displayedInspectorTargetAtom, { kind: 'scope', nodeId: snapshot.nodeId });
  store.set(inspectorLoadingAtom, false);
  store.set(inspectorErrorAtom, null);
}

export async function loadInspectorData(entityHash: string | null) {
  const v = ++loadVersion;

  if (!entityHash) {
    commitScopeFromSnapshot();
    return;
  }

  store.set(inspectorLoadingAtom, true);
  store.set(inspectorErrorAtom, null);

  try {
    const result = await api.getEntityDetails(entityHash);
    if (v !== loadVersion || !result) return;
    await preloadImage(`media://localhost/thumb/${result.entity_hash}.jpg`);
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

function syncToDisplayedState(target: InspectorTarget) {
  if (target.kind === 'entity') {
    void loadInspectorData(target.entityHash);
    return;
  }

  // Scope/none mode is driven by the committed displayed grid snapshot.
  if (store.get(liveInspectorTargetAtom).kind !== 'entity') {
    loadVersion++;
    commitScopeFromSnapshot();
  }
}

export function startInspectorSync() {
  if (subscribed) return;
  subscribed = true;

  let lastKey = '';
  const runSync = () => {
    const liveTarget = store.get(liveInspectorTargetAtom);
    const snapshot = store.get(displayedGridSnapshotAtom);
    const key =
      liveTarget.kind === 'entity'
        ? `entity:${liveTarget.entityHash}`
        : snapshot
          ? `scope:${snapshot.nodeId}`
          : liveTarget.kind;
    if (key === lastKey) return;
    lastKey = key;
    syncToDisplayedState(liveTarget);
  };

  runSync();
  store.sub(liveInspectorTargetAtom, runSync);
  store.sub(displayedGridSnapshotAtom, runSync);
}
