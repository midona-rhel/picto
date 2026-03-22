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
      // Controllers perform targeted sidebar mutations eagerly (patchFolderNode,
      // removeFolderNode, adjustFolderCount, insertFolderNode, etc.).
      // The sidebar/tree target is consumed without a broad refetch.
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
