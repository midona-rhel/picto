import { getDefaultStore } from 'jotai';
import { listen } from '../platform/ipc';
import { loadInspectorData } from '../controllers/inspectorController';
import {
  displayedInspectorEntityDataAtom,
  displayedInspectorTargetAtom,
  inspectorErrorAtom,
  inspectorLoadingAtom,
  inspectorPinnedAtom,
  liveInspectorTargetAtom,
} from '../state/inspector';

const store = getDefaultStore();

export function startInspectorSettle(): () => void {
  let cancelled = false;
  let lastEntityHash = '';

  const unsubTarget = store.sub(liveInspectorTargetAtom, () => {
    if (cancelled) return;
    if (store.get(inspectorPinnedAtom)) return;

    const target = store.get(liveInspectorTargetAtom);
    if (target.kind === 'entity') {
      if (target.entityHash !== lastEntityHash) {
        lastEntityHash = target.entityHash;
        void loadInspectorData(target.entityHash);
      }
      return;
    }

    // For scope/multi/none targets: only commit immediately for entity→scope transitions.
    // Scope→scope transitions are handled by GridScreen's snapshot commit to avoid
    // flashing partial data (the inspector would show the new scope node before the
    // grid query returns size/count).
    if (lastEntityHash) {
      lastEntityHash = '';
      store.set(displayedInspectorEntityDataAtom, null);
      store.set(displayedInspectorTargetAtom, target);
      store.set(inspectorLoadingAtom, false);
      store.set(inspectorErrorAtom, null);
    }
  });

  const unlistenPromise = listen<{ changes: Record<string, unknown>; seq?: number }>(
    'runtime/state_changed',
    (event) => {
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
    },
  );

  return () => {
    cancelled = true;
    unsubTarget();
    unlistenPromise.then((fn) => fn()).catch(() => {});
  };
}
