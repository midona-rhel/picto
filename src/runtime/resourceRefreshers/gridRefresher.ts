import { useRuntimeSyncStore } from '../../state/runtimeSyncStore';
import { useCacheStore } from '../../state/cacheStore';
import { invalidateMetadata } from '#features/grid/data';
import { gridResourceMatchesScope } from '../resourceInvalidator';
import type { ResourceKey } from '../../shared/types/generated/runtime-contract';

let unsub: (() => void) | null = null;
let prevStaleRef: Set<unknown> | null = null;

export function startGridRefresher(): void {
  if (unsub) return;

  unsub = useRuntimeSyncStore.subscribe((state) => {
    if (state.staleResources === prevStaleRef) return;
    prevStaleRef = state.staleResources;

    const activeScope = useCacheStore.getState().activeGridScope;
    const originCommand = state.lastOriginCommand;
    const toFreshen: ResourceKey[] = [];

    for (const key of state.staleResources) {
      // Metadata hash invalidation
      if (key.startsWith('metadata/hash:')) {
        const hash = key.slice('metadata/hash:'.length);
        useCacheStore.getState().invalidateHash(hash);
        useCacheStore.getState().markHashInvalidated(hash);
        invalidateMetadata(hash);
        toFreshen.push(key);
        continue;
      }

      // Grid scope invalidation
      if (key.startsWith('grid/')) {
        const matches = gridResourceMatchesScope(key, activeScope);

        // Subscription import suppression
        const skipInboxReplace =
          activeScope === 'system:inbox'
          && originCommand === 'subscription_import'
          && key === 'grid/system:inbox';

        if (matches && !skipInboxReplace) {
          useCacheStore.getState().invalidateAll();
          useCacheStore.getState().bumpGridRefresh();
        }
        toFreshen.push(key);
      }
    }

    for (const key of toFreshen) {
      useRuntimeSyncStore.getState().markResourceFresh(key);
    }
  });
}

export function stopGridRefresher(): void {
  if (unsub) {
    unsub();
    unsub = null;
  }
  prevStaleRef = null;
}
