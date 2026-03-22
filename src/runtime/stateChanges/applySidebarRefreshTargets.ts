import { useStateChangeStore } from './stateChangeStore';
import { useDomainStore } from '../../state/domainStore';

let unsub: (() => void) | null = null;
let prevStaleRef: Set<unknown> | null = null;

export function startApplyingSidebarRefreshTargets(): void {
  if (unsub) return;

  unsub = useStateChangeStore.subscribe((state) => {
    // Only react when the pending target set identity changes
    if (state.pendingRefreshTargets === prevStaleRef) return;
    prevStaleRef = state.pendingRefreshTargets;

    const pendingRefreshTargets = state.pendingRefreshTargets;
    let needsTreeRefresh = false;
    let needsCountsRefresh = false;

    for (const key of pendingRefreshTargets) {
      if (key === 'sidebar/tree') needsTreeRefresh = true;
      if (key === 'sidebar/counts') needsCountsRefresh = true;
    }

    if (needsCountsRefresh) {
      const counts = useStateChangeStore.getState().sidebarCounts;
      if (counts) {
        useDomainStore.getState().applySidebarCounts(counts);
      }
      useStateChangeStore.getState().markRefreshTargetHandled('sidebar/counts');
    }

    if (needsTreeRefresh) {
      // Use the same requestRefresh() as controller eager updates so both
      // paths share the same 120ms debounce timer — no double-fetch.
      useDomainStore.getState().requestRefresh();
      useStateChangeStore.getState().markRefreshTargetHandled('sidebar/tree');
    }
  });
}

export function stopApplyingSidebarRefreshTargets(): void {
  if (unsub) {
    unsub();
    unsub = null;
  }
  prevStaleRef = null;
}
