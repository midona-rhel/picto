import { useStateChangeStore } from './stateChangeStore';
import { useGridMetadataStore } from '../../state/gridMetadataStore';
import { noteMetadataChanged } from '#features/grid/data';
import { filesController } from '../../controllers/filesController';
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

    const handledTargets: ResourceKey[] = [];
    const activeScope = useGridMetadataStore.getState().activeGridScope;

    // Phase 1: Collect hashes not already handled eagerly by controllers.
    const newHashes: string[] = [];
    let hasMatchingGridScope = false;

    for (const key of state.pendingRefreshTargets) {
      if (key.startsWith('metadata/hash:')) {
        const hash = key.slice('metadata/hash:'.length);
        if (!isRecentlyEagerInvalidated(hash)) {
          newHashes.push(hash);
        }
      }
      if (key.startsWith('grid/') && refreshTargetMatchesGridScope(key, activeScope)) {
        hasMatchingGridScope = true;
      }
    }

    // Phase 2: Apply targets.
    for (const key of state.pendingRefreshTargets) {
      if (key.startsWith('metadata/hash:')) {
        const hash = key.slice('metadata/hash:'.length);
        if (!isRecentlyEagerInvalidated(hash)) {
          useGridMetadataStore.getState().dropCachedMetadata(hash);
          useGridMetadataStore.getState().markMetadataChanged(hash);
          noteMetadataChanged(hash);
        }
        handledTargets.push(key);
        continue;
      }

      if (key.startsWith('grid/')) {
        // When a grid scope matches AND there are new hashes that no
        // controller handled eagerly, fetch those entities and insert
        // them into the grid. This covers background producers like
        // subscription imports and watch-folder imports where no
        // frontend controller is involved.
        if (hasMatchingGridScope && newHashes.length > 0) {
          const hashesToInsert = [...newHashes];
          newHashes.length = 0; // only insert once per batch
          Promise.all(hashesToInsert.map((h) => filesController.getEntity(h))).then((entities) => {
            const valid = entities.filter((e): e is NonNullable<typeof e> => e != null);
            if (valid.length > 0) {
              useGridMetadataStore.getState().queueInsertions(valid);
            }
          });
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
