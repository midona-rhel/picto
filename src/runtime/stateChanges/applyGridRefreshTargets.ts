import { useStateChangeStore } from './stateChangeStore';
import { useGridMetadataStore } from '../../state-legacy/gridMetadataStore';
import { noteMetadataChanged } from '#features/grid/data';
import { entityController } from '../../controllers/entityController';
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

    // Diagnostic: log every state change that reaches the applier
    const origin = state.lastChangeOrigin;
    const gridKeys = [...state.pendingRefreshTargets].filter((k) => k.startsWith('grid/'));
    const hashKeys = [...state.pendingRefreshTargets].filter((k) => k.startsWith('metadata/hash:'));
    if (gridKeys.length > 0 || hashKeys.length > 0) {
      console.log(
        `[grid-applier] origin=${origin} activeScope=${activeScope} gridKeys=${gridKeys.length} hashKeys=${hashKeys.length} insertionScopes=${JSON.stringify(state.lastInsertionScopes)}`,
      );
    }

    // Phase 1: Collect hashes not already handled eagerly by controllers.
    const newHashes: string[] = [];

    for (const key of state.pendingRefreshTargets) {
      if (key.startsWith('metadata/hash:')) {
        const hash = key.slice('metadata/hash:'.length);
        if (!isRecentlyEagerInvalidated(hash)) {
          newHashes.push(hash);
        }
      }
    }

    // Check if the current grid scope matches the backend's explicit insertion
    // scopes (extra_grid_scopes). This is the ONLY signal for whether new tiles
    // should be eagerly inserted — NOT the broader set from status_changed.
    const insertionScopes = state.lastInsertionScopes;
    const scopeMatchesInsertion = insertionScopes != null && activeScope != null
      && insertionScopes.some((scope) =>
        refreshTargetMatchesGridScope(`grid/${scope}`, activeScope),
      );

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
        // Only insert new tiles for background producers (subscription/watch
        // imports) where no frontend controller handled the eager update.
        // Controller-initiated actions (status change, folder drag, etc.)
        // already did their own eager insertions/removals — don't re-insert
        // expanded collection members as individual tiles.
        const changeOrigin = state.lastChangeOrigin;
        const isBackgroundOrigin =
          changeOrigin === 'subscription_import'
          || changeOrigin === 'subscription_collection_import'
          || changeOrigin === 'watch_folder_import'
          || changeOrigin === 'watch_folder_membership'
          || changeOrigin === 'manual_import';
        if (isBackgroundOrigin && newHashes.length > 0) {
          if (scopeMatchesInsertion) {
            const hashesToInsert = [...newHashes];
            newHashes.length = 0;
            console.log(
              `[grid-insert] Inserting ${hashesToInsert.length} tile(s) — origin=${changeOrigin} activeScope=${activeScope} insertionScopes=${JSON.stringify(insertionScopes)}`,
            );
            entityController.getGridItems(hashesToInsert).then((entities) => {
              if (entities.length > 0) {
                useGridMetadataStore.getState().queueInsertions(entities);
              }
            });
          } else {
            console.log(
              `[grid-insert] BLOCKED ${newHashes.length} tile(s) — scope mismatch: origin=${changeOrigin} activeScope=${activeScope} insertionScopes=${JSON.stringify(insertionScopes)}`,
            );
          }
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
