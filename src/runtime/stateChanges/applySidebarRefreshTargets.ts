import { useStateChangeStore } from './stateChangeStore';
import { useDomainStore } from '../../state/domainStore';

let unsub: (() => void) | null = null;
let debounceTimer: ReturnType<typeof setTimeout> | null = null;
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
      console.log('[sidebar-refresh] sidebar/counts queued, applying', counts);
      if (counts) {
        useDomainStore.getState().applySidebarCounts(counts);
      }
      useStateChangeStore.getState().markRefreshTargetHandled('sidebar/counts');
    }

    if (needsTreeRefresh) {
      console.log('[sidebar-refresh] sidebar/tree queued, scheduling refresh');
      if (debounceTimer) clearTimeout(debounceTimer);
      debounceTimer = setTimeout(() => {
        useDomainStore.getState().requestRefresh();
        useStateChangeStore.getState().markRefreshTargetHandled('sidebar/tree');
        debounceTimer = null;
      }, 120);
    }
  });
}

export function stopApplyingSidebarRefreshTargets(): void {
  if (unsub) {
    unsub();
    unsub = null;
  }
  if (debounceTimer) {
    clearTimeout(debounceTimer);
    debounceTimer = null;
  }
  prevStaleRef = null;
}
