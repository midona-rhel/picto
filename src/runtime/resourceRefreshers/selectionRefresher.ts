import { useRuntimeSyncStore } from '../../state/runtimeSyncStore';
import { invalidateSelectionSummary } from '#features/grid/data';

let unsub: (() => void) | null = null;
let prevStaleRef: Set<unknown> | null = null;

export function startSelectionRefresher(): void {
  if (unsub) return;

  unsub = useRuntimeSyncStore.subscribe((state) => {
    if (state.staleResources === prevStaleRef) return;
    prevStaleRef = state.staleResources;

    if (state.staleResources.has('selection/current')) {
      invalidateSelectionSummary();
      useRuntimeSyncStore.getState().markResourceFresh('selection/current');
    }
  });
}

export function stopSelectionRefresher(): void {
  if (unsub) {
    unsub();
    unsub = null;
  }
  prevStaleRef = null;
}
