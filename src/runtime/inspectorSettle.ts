import { getDefaultStore } from 'jotai';
import { listen } from '../platform/ipc';
import {
  commitInspectorTarget,
  loadInspectorData,
  loadSubfolderInspectorPreview,
} from '../controllers/inspectorController';
import {
  displayedGridSnapshotAtom,
  displayedInspectorEntityDataAtom,
  displayedInspectorTargetAtom,
  inspectorPinnedAtom,
  liveInspectorTargetAtom,
} from '../state/inspector';
import { selectedSubfolderNodeIdAtom } from '../state/selection';

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

    const selectedSubfolderNodeId = store.get(selectedSubfolderNodeIdAtom);
    if (target.kind === 'scope' && selectedSubfolderNodeId === target.nodeId) {
      lastEntityHash = '';
      void loadSubfolderInspectorPreview(target.nodeId);
      return;
    }

    // For scope/multi/none targets: only commit immediately for entity→scope transitions.
    // Scope→scope transitions are handled by GridScreen's snapshot commit to avoid
    // flashing partial data (the inspector would show the new scope node before the
    // grid query returns size/count).
    const displayedTarget = store.get(displayedInspectorTargetAtom);
    const displayedSnapshot = store.get(displayedGridSnapshotAtom);
    const returningToDisplayedScope = target.kind === 'scope'
      && displayedSnapshot?.nodeId === target.nodeId
      && displayedTarget.kind === 'scope'
      && displayedTarget.nodeId !== target.nodeId;
    if (lastEntityHash || returningToDisplayedScope) {
      lastEntityHash = '';
      commitInspectorTarget(target);
    }
  });

  const unlistenPromise = listen<{ changes: Record<string, unknown>; seq?: number }>(
    'runtime/state_changed',
    (event) => {
      if (cancelled) return;

      const changes = event.payload.changes;
      const relevant = changes.tags_changed
        || changes.tag_structure_changed
        || changes.status_changed
        || changes.folder_membership_changed
        || changes.media_metadata_changed;
      if (!relevant) return;

      const data = store.get(displayedInspectorEntityDataAtom);
      if (!data) return;

      const hashes = changes.entity_hashes as string[] | undefined;
      if (!changes.tag_structure_changed
          && hashes?.length
          && !hashes.includes(data.entity_hash)) return;

      void loadInspectorData(data.entity_hash);
    },
  );

  return () => {
    cancelled = true;
    unsubTarget();
    unlistenPromise.then((fn) => fn()).catch(() => {});
  };
}
