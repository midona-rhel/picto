/**
 * Grid runtime settle — scope-aware event filtering with backend reconcile.
 *
 * Decision model:
 *   1. If grid is not the active surface → ignore
 *   2. extra_grid_scopes is authoritative when present
 *   3. Metadata/derivative-only changes for known entity hashes → reconcile
 *   4. Membership changes (status, folder, tags) → reconcile (backend decides)
 *   5. Compiler batch → full refresh (fallback)
 *   6. Unknown → ignore
 *
 * Events arriving during grid transitions (fading_out / waiting) are queued
 * and replayed once the transition settles.
 */

import { getDefaultStore } from 'jotai';
import { listen } from '../platform/ipc';
import { gridScopeAtom, gridActiveAtom, gridItemsAtom, gridTransitionPhaseAtom } from '../state/grid';
import { gridController } from '../controllers/gridController';
import type { BaseScope } from '../shared/types/canonical';

const store = getDefaultStore();

interface StateChanges {
  domains?: string[];
  entity_hashes?: string[];
  folder_ids?: number[];
  smart_folder_ids?: number[];
  compiler_batch_done?: boolean;
  status_changed?: boolean;
  tags_changed?: boolean;
  folder_membership_changed?: number[];
  media_metadata_changed?: boolean;
  media_derivatives_changed?: boolean;
  derivative_fields_changed?: string[];
  extra_grid_scopes?: string[];
}

function scopeToKey(scope: BaseScope): string | null {
  switch (scope.kind) {
    case 'system': return `system:${scope.key === 'all' ? 'active' : scope.key}`;
    case 'folder': return scope.id != null ? `folder:${scope.id}` : null;
    case 'smart_folder': return scope.id != null ? `smart:${scope.id}` : null;
    case 'collection': return scope.id != null ? `collection:${scope.id}` : null;
    default: return null;
  }
}

type GridAction =
  | 'ignore'
  | 'reconcile_metadata'
  | 'reconcile_membership'
  | 'refresh';

function classifyGridAction(changes: StateChanges, scope: BaseScope): GridAction {
  if (changes.extra_grid_scopes?.length) {
    const currentKey = scopeToKey(scope);
    if (currentKey && changes.extra_grid_scopes.includes(currentKey)) {
      const membershipChanged = !!(changes.status_changed
        || changes.folder_membership_changed?.length
        || changes.tags_changed);
      return membershipChanged ? 'reconcile_membership' : 'reconcile_metadata';
    }
    return 'ignore';
  }

  if (changes.entity_hashes?.length && !changes.status_changed
      && !changes.folder_membership_changed?.length && !changes.tags_changed) {
    const visible = new Set(store.get(gridItemsAtom).map((i) => i.entity_hash));
    if (changes.entity_hashes.some((h) => visible.has(h))) {
      return 'reconcile_metadata';
    }
    return 'ignore';
  }

  if (changes.status_changed && scope.kind === 'system') {
    return 'reconcile_membership';
  }

  if (changes.folder_membership_changed?.length && scope.kind === 'folder') {
    if (scope.id != null && changes.folder_membership_changed.includes(scope.id)) {
      return 'reconcile_membership';
    }
    return 'ignore';
  }

  if (changes.smart_folder_ids?.length && scope.kind === 'smart_folder') {
    if (scope.id != null && changes.smart_folder_ids.includes(scope.id)) {
      return 'reconcile_membership';
    }
    return 'ignore';
  }

  if (changes.tags_changed && scope.kind === 'system' && scope.key === 'untagged') {
    return 'reconcile_membership';
  }

  if (changes.media_derivatives_changed && !changes.status_changed
      && !changes.folder_membership_changed?.length) {
    return 'ignore';
  }

  if (changes.compiler_batch_done) {
    return 'ignore';
  }

  if (changes.entity_hashes?.length && scope.kind === 'system') {
    return 'reconcile_membership';
  }

  return 'ignore';
}

function processStateChange(changes: StateChanges) {
  const scope = store.get(gridScopeAtom);
  const action = classifyGridAction(changes, scope);

  switch (action) {
    case 'ignore':
      break;
    case 'reconcile_metadata':
      gridController.reconcile(true);
      break;
    case 'reconcile_membership': {
      const hashes = changes.entity_hashes;
      if (hashes?.length) {
        const visible = new Set(store.get(gridItemsAtom).map((i) => i.entity_hash));
        const newHashes = hashes.filter((h) => !visible.has(h));
        const existingHashes = hashes.filter((h) => visible.has(h));

        if (newHashes.length > 0 && existingHashes.length > 0 && changes.status_changed && scope.kind === 'system') {
          gridController.removeItems(existingHashes);
          gridController.insertItems(newHashes);
          break;
        }

        if (newHashes.length > 0) {
          gridController.insertItems(newHashes);
          break;
        }

        if (changes.status_changed && scope.kind === 'system'
            && hashes.every((h) => visible.has(h))) {
          gridController.removeItems(hashes);
          break;
        }
      }
      gridController.reconcile(false);
      break;
    }
    case 'refresh':
      gridController.loadFirstPage({ preserveItems: true });
      break;
  }
}

/**
 * Start the grid settle listener. Returns a cleanup function that
 * cancels the listener (HMR safety — call before re-registering).
 */
export function startGridSettle(): () => void {
  let cancelled = false;
  let pendingChanges: StateChanges | null = null;

  // When transition settles, replay any queued event
  const unsubPhase = store.sub(gridTransitionPhaseAtom, () => {
    if (cancelled) return;
    const phase = store.get(gridTransitionPhaseAtom);
    if ((phase === 'idle' || phase === 'fading_in') && pendingChanges) {
      const changes = pendingChanges;
      pendingChanges = null;
      if (store.get(gridActiveAtom)) processStateChange(changes);
    }
  });

  const unlistenPromise = listen<{ changes: StateChanges; seq?: number }>('runtime/state_changed', (event) => {
    if (cancelled) return;
    if (!store.get(gridActiveAtom)) return;

    const phase = store.get(gridTransitionPhaseAtom);
    if (phase === 'fading_out' || phase === 'waiting') {
      // Queue the latest event — will be replayed when transition settles
      pendingChanges = event.payload.changes;
      return;
    }

    processStateChange(event.payload.changes);
  });

  return () => {
    cancelled = true;
    unsubPhase();
    unlistenPromise.then((fn) => fn()).catch(() => {});
  };
}
