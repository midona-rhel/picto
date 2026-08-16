/**
 * Grid runtime settle — scope-aware event filtering with backend reconcile.
 *
 * Decision model:
 *   1. If grid is not the active surface → ignore
 *   2. Canonical status/folder/smart-folder facts settle their matching scope
 *   3. extra_grid_scopes settles additional explicitly affected scopes
 *   4. Metadata/derivative-only changes for known entity hashes → reconcile
 *   5. Compiler batch → full refresh (fallback)
 *   6. Unknown → ignore
 *
 * Events arriving during grid transitions (fading_out / waiting) are queued
 * and replayed once the transition settles.
 */

import { getDefaultStore } from 'jotai';
import { listen } from '../platform/ipc';
import { gridActiveAtom, gridReconcileContextAtom, gridTransitionPhaseAtom } from '../state/grid';
import { gridController } from '../controllers/gridController';
import type { BaseScope } from '../shared/types/canonical';
import { scopeToGridNodeId } from '../shared/lib/gridScope';

const store = getDefaultStore();

interface StateChanges {
  domains?: string[];
  entity_hashes?: string[];
  folder_ids?: number[];
  smart_folder_ids?: number[];
  compiler_batch_done?: boolean;
  status_changed?: boolean;
  tags_changed?: boolean;
  tag_structure_changed?: boolean;
  folder_membership_changed?: number[];
  media_metadata_changed?: boolean;
  media_derivatives_changed?: boolean;
  derivative_fields_changed?: string[];
  extra_grid_scopes?: string[];
  grid_reorder?: boolean;
}

function scopeToKey(scope: BaseScope): string | null {
  return scopeToGridNodeId(scope);
}

type GridAction =
  | 'ignore'
  | 'reconcile_metadata'
  | 'reconcile_membership'
  | 'reorder'
  | 'refresh';

export function classifyGridAction(
  changes: StateChanges,
  scope: BaseScope,
  visibleEntityHashes: string[],
): GridAction {
  if (changes.tag_structure_changed) {
    return 'reconcile_membership';
  }

  if (changes.status_changed) {
    return 'reconcile_membership';
  }

  if (changes.folder_membership_changed?.length && scope.kind === 'folder') {
    if (scope.id != null && changes.folder_membership_changed.includes(scope.id)) {
      return 'reconcile_membership';
    }
  }

  if (changes.smart_folder_ids?.length && scope.kind === 'smart_folder') {
    if (scope.id != null && changes.smart_folder_ids.includes(scope.id)) {
      return 'reconcile_membership';
    }
  }

  if (changes.extra_grid_scopes?.length) {
    const currentKey = scopeToKey(scope);
    if (currentKey && changes.extra_grid_scopes.includes(currentKey)) {
      if (changes.grid_reorder) return 'reorder';
      const membershipChanged = !!(changes.status_changed
        || changes.folder_membership_changed?.length
        || changes.tags_changed
        || changes.smart_folder_ids?.length);
      return membershipChanged ? 'reconcile_membership' : 'reconcile_metadata';
    }
    return 'ignore';
  }

  if (changes.entity_hashes?.length && !changes.status_changed
      && !changes.folder_membership_changed?.length && !changes.tags_changed) {
    const visible = new Set(visibleEntityHashes);
    if (changes.entity_hashes.some((h) => visible.has(h))) {
      return 'reconcile_metadata';
    }
    return 'ignore';
  }

  if (changes.folder_membership_changed?.length && scope.kind === 'folder') {
    return 'ignore';
  }

  if (changes.smart_folder_ids?.length && scope.kind === 'smart_folder') {
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

export function processStateChange(changes: StateChanges) {
  const { scope, visibleEntityHashes } = store.get(gridReconcileContextAtom);
  const action = classifyGridAction(changes, scope, visibleEntityHashes);

  switch (action) {
    case 'ignore':
      break;
    case 'reconcile_metadata':
      gridController.reconcile(true);
      break;
    case 'reconcile_membership':
      // Membership changes must be settled by the canonical query. Hash-only
      // insertion cannot know whether the current scope contains the entity.
      gridController.loadFirstPage({ preserveItems: true });
      break;
    case 'reorder':
      gridController.reconcile(false);
      break;
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
  let pendingReload = false;

  // Coalesce every event during a transition into one canonical reload. Keeping
  // only the latest payload can lose a relevant event behind an unrelated one.
  const unsubPhase = store.sub(gridTransitionPhaseAtom, () => {
    if (cancelled) return;
    const phase = store.get(gridTransitionPhaseAtom);
    if ((phase === 'idle' || phase === 'fading_in') && pendingReload) {
      pendingReload = false;
      if (store.get(gridActiveAtom)) {
        gridController.loadFirstPage({ preserveItems: true });
      }
    }
  });

  const unlistenPromise = listen<{ changes: StateChanges; seq?: number }>('runtime/state_changed', (event) => {
    if (cancelled) return;
    if (!store.get(gridActiveAtom)) return;

    const phase = store.get(gridTransitionPhaseAtom);
    if (phase === 'fading_out' || phase === 'waiting') {
      pendingReload = true;
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
