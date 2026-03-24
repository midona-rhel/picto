import { useStateChangeStore } from './stateChangeStore';
import { store as jotaiStore } from '../../state/store';
import { applySidebarCountsAtom } from '../../state/sidebar';
import { useDomainStore } from '../../state-legacy/domainStore';

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
        jotaiStore.set(applySidebarCountsAtom, counts);
      }
      useStateChangeStore.getState().markRefreshTargetHandled('sidebar/counts');
    }

    if (needsTreeRefresh) {
      const changeOrigin = useStateChangeStore.getState().lastChangeOrigin;
      if (changeOrigin === 'compiler_batch_done') {
        // Compiler rebuilt counts (uncategorized, untagged, smart folders, etc.)
        // that no controller handles eagerly. Re-fetch the sidebar tree.
        useDomainStore.getState().requestRefresh();
      }
      // For controller-initiated changes, the controller already did targeted
      // sidebar mutations (patchFolderNode, adjustFolderCount, etc.).
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
