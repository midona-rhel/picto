import { useStateChangeStore } from './stateChangeStore';
import { useGridMetadataStore } from '../../state/gridMetadataStore';
import { noteMetadataChanged } from '#features/grid/data';
import { refreshTargetMatchesGridScope } from './planRefreshTargets';
import type { ResourceKey } from '../../shared/types/backendState';

let unsub: (() => void) | null = null;
let prevStaleRef: Set<unknown> | null = null;

// Track hashes recently invalidated by controller eager updates so backend
// events arriving within the reconciliation window don't re-fetch the same
// metadata. Uses a TTL rather than metadataInvalidatedHashes because React
// clears that set on render (before the backend event arrives).
const recentlyEagerInvalidated = new Map<string, number>();
const EAGER_RECONCILE_WINDOW_MS = 500;

export function markEagerInvalidated(hash: string): void {
  recentlyEagerInvalidated.set(hash, Date.now());
}

function isRecentlyEagerInvalidated(hash: string): boolean {
  const ts = recentlyEagerInvalidated.get(hash);
  if (!ts) return false;
  if (Date.now() - ts > EAGER_RECONCILE_WINDOW_MS) {
    recentlyEagerInvalidated.delete(hash);
    return false;
  }
  return true;
}

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
        // Skip re-invalidation if the eager controller path already
        // invalidated this hash within the reconciliation window.
        if (!isRecentlyEagerInvalidated(hash)) {
          useGridMetadataStore.getState().dropCachedMetadata(hash);
          useGridMetadataStore.getState().markMetadataChanged(hash);
          noteMetadataChanged(hash);
        }
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
  recentlyEagerInvalidated.clear();
}
