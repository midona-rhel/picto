import { useStateChangeStore } from './stateChangeStore';
import { noteSelectionSummaryChanged } from '#features/grid/data';

let unsub: (() => void) | null = null;
let prevStaleRef: Set<unknown> | null = null;

export function startApplyingSelectionRefreshTargets(): void {
  if (unsub) return;

  unsub = useStateChangeStore.subscribe((state) => {
    if (state.pendingRefreshTargets === prevStaleRef) return;
    prevStaleRef = state.pendingRefreshTargets;

    if (state.pendingRefreshTargets.has('selection/current')) {
      noteSelectionSummaryChanged();
      useStateChangeStore.getState().markRefreshTargetHandled('selection/current');
    }
  });
}

export function stopApplyingSelectionRefreshTargets(): void {
  if (unsub) {
    unsub();
    unsub = null;
  }
  prevStaleRef = null;
}
