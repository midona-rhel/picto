import { useDomainStore } from '../state/domainStore';

/**
 * SidebarController — orchestration facade for sidebar tree fetch/invalidation.
 *
 * Keeps sidebar workflow calls out of components/event listeners so ownership
 * stays centralized as V2 state management is completed.
 */
let refreshQueued = false;

function scheduleOncePerFrame(fn: () => void): void {
  if (typeof window !== 'undefined' && typeof window.requestAnimationFrame === 'function') {
    window.requestAnimationFrame(fn);
    return;
  }
  setTimeout(fn, 0);
}

export const SidebarController = {
  fetchInitialTree(): Promise<void> {
    return useDomainStore.getState().fetchSidebarTree();
  },

  requestRefresh(): void {
    if (refreshQueued) return;
    refreshQueued = true;
    scheduleOncePerFrame(() => {
      refreshQueued = false;
      useDomainStore.getState().invalidate();
    });
  },
};
