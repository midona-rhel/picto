import { getDefaultStore } from 'jotai';
import { loadInspectorData } from '../controllers/inspectorController';
import {
  displayedInspectorItemDetailsAtom,
  displayedInspectorTargetAtom,
  inspectorErrorAtom,
  inspectorLoadingAtom,
  inspectorPinnedAtom,
  liveInspectorTargetAtom,
} from '../state/inspector';
import { libraryInvalidation } from './libraryInvalidation';

const store = getDefaultStore();

export function startInspectorSettle(): () => void {
  let cancelled = false;
  let lastItemId: number | null = null;

  const unregisterInvalidation = libraryInvalidation.register('library', () => {
    if (cancelled) return;
    const itemId = store.get(displayedInspectorItemDetailsAtom)?.item_id ?? lastItemId;
    if (itemId != null) void loadInspectorData(itemId);
  });

  const unsubTarget = store.sub(liveInspectorTargetAtom, () => {
    if (cancelled) return;
    if (store.get(inspectorPinnedAtom)) return;

    const target = store.get(liveInspectorTargetAtom);
    if (target.kind === 'item') {
      if (target.itemId !== lastItemId) {
        lastItemId = target.itemId;
        void loadInspectorData(target.itemId);
      }
      return;
    }

    // For scope/multi/none targets: only commit immediately for entity→scope transitions.
    // Scope→scope transitions are handled by GridScreen's snapshot commit to avoid
    // flashing partial data (the inspector would show the new scope node before the
    // grid query returns size/count).
    if (lastItemId != null) {
      lastItemId = null;
      store.set(displayedInspectorItemDetailsAtom, null);
      store.set(displayedInspectorTargetAtom, target);
      store.set(inspectorLoadingAtom, false);
      store.set(inspectorErrorAtom, null);
    }
  });

  return () => {
    cancelled = true;
    unsubTarget();
    unregisterInvalidation();
  };
}
