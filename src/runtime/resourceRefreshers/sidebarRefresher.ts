import { useRuntimeSyncStore } from '../../stores/runtimeSyncStore';
import { useDomainStore } from '../../stores/domainStore';
import { SidebarController } from '../../controllers/sidebarController';

let unsub: (() => void) | null = null;
let debounceTimer: ReturnType<typeof setTimeout> | null = null;
let prevStaleRef: Set<unknown> | null = null;

export function startSidebarRefresher(): void {
  if (unsub) return;

  unsub = useRuntimeSyncStore.subscribe((state) => {
    // Only react when staleResources identity changes
    if (state.staleResources === prevStaleRef) return;
    prevStaleRef = state.staleResources;

    const staleResources = state.staleResources;
    let needsTreeRefresh = false;
    let needsCountsRefresh = false;

    for (const key of staleResources) {
      if (key === 'sidebar/tree') needsTreeRefresh = true;
      if (key === 'sidebar/counts') needsCountsRefresh = true;
    }

    if (needsCountsRefresh) {
      const counts = useRuntimeSyncStore.getState().sidebarCounts;
      if (counts) {
        useDomainStore.getState().applySidebarCounts(counts);
      }
      useRuntimeSyncStore.getState().markResourceFresh('sidebar/counts');
    }

    if (needsTreeRefresh) {
      if (debounceTimer) clearTimeout(debounceTimer);
      debounceTimer = setTimeout(() => {
        SidebarController.requestRefresh();
        useRuntimeSyncStore.getState().markResourceFresh('sidebar/tree');
        debounceTimer = null;
      }, 120);
    }
  });
}

export function stopSidebarRefresher(): void {
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
