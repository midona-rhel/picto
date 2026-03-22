import { useStateChangeStore } from './stateChangeStore';
import { useGridMetadataStore } from '../../state/gridMetadataStore';
import { noteMetadataChanged } from '#features/grid/data';
import { refreshTargetMatchesGridScope } from './planRefreshTargets';
import type { ResourceKey } from '../../shared/types/backendState';

let unsub: (() => void) | null = null;
let prevStaleRef: Set<unknown> | null = null;

export function startApplyingGridRefreshTargets(): void {
  if (unsub) return;

  unsub = useStateChangeStore.subscribe((state) => {
    if (state.pendingRefreshTargets === prevStaleRef) return;
    prevStaleRef = state.pendingRefreshTargets;

    const activeScope = useGridMetadataStore.getState().activeGridScope;
    const changeOrigin = state.lastChangeOrigin;
    const handledTargets: ResourceKey[] = [];

    for (const key of state.pendingRefreshTargets) {
      // Metadata hash refresh target
      if (key.startsWith('metadata/hash:')) {
        const hash = key.slice('metadata/hash:'.length);
        useGridMetadataStore.getState().dropCachedMetadata(hash);
        useGridMetadataStore.getState().markMetadataChanged(hash);
        noteMetadataChanged(hash);
        handledTargets.push(key);
        continue;
      }

      // Grid scope refresh target
      if (key.startsWith('grid/')) {
        const matches = refreshTargetMatchesGridScope(key, activeScope);

        // Subscription import suppression
        const skipInboxReplace =
          activeScope === 'system:inbox'
          && changeOrigin === 'subscription_import'
          && key === 'grid/system:inbox';

        if (matches && !skipInboxReplace) {
          useGridMetadataStore.getState().clearMetadataCache();
          useGridMetadataStore.getState().bumpGridRefresh();
        }
        handledTargets.push(key);
      }
    }

    for (const key of handledTargets) {
      useStateChangeStore.getState().markRefreshTargetHandled(key);
    }
  });
}

export function stopApplyingGridRefreshTargets(): void {
  if (unsub) {
    unsub();
    unsub = null;
  }
  prevStaleRef = null;
}
