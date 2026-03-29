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
 * Three response levels:
 *   - ignore: event does not affect current grid
 *   - reconcile: ask backend if visible rows changed (may patch or refresh)
 *   - refresh: reload the grid page (fallback)
 */

import { getDefaultStore } from 'jotai';
import { listen } from '../platform/ipc';
import { gridScopeAtom, gridActiveAtom, gridItemsAtom } from '../state/grid';
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
  | 'reconcile_metadata'  // metadata/derivative only — safe to patch visible rows
  | 'reconcile_membership' // membership may have changed — backend will decide (likely refresh)
  | 'refresh';             // full reload

function classifyGridAction(changes: StateChanges, scope: BaseScope): GridAction {
  // 1. extra_grid_scopes is AUTHORITATIVE
  if (changes.extra_grid_scopes?.length) {
    const currentKey = scopeToKey(scope);
    if (currentKey && changes.extra_grid_scopes.includes(currentKey)) {
      // Scope is affected — but was it metadata-only or membership?
      const membershipChanged = !!(changes.status_changed
        || changes.folder_membership_changed?.length
        || changes.tags_changed);
      return membershipChanged ? 'reconcile_membership' : 'reconcile_metadata';
    }
    return 'ignore';
  }

  // 2. Metadata/derivative-only with entity hashes — safe to patch if visible
  if (changes.entity_hashes?.length && !changes.status_changed
      && !changes.folder_membership_changed?.length && !changes.tags_changed) {
    const visible = new Set(store.get(gridItemsAtom).map((i) => i.entity_hash));
    if (changes.entity_hashes.some((h) => visible.has(h))) {
      return 'reconcile_metadata';
    }
    return 'ignore';
  }

  // 3. Status changes → membership change for system scopes
  if (changes.status_changed && scope.kind === 'system') {
    return 'reconcile_membership';
  }

  // 4. Folder membership → membership change
  if (changes.folder_membership_changed?.length && scope.kind === 'folder') {
    if (scope.id != null && changes.folder_membership_changed.includes(scope.id)) {
      return 'reconcile_membership';
    }
    return 'ignore';
  }

  // 5. Smart folder bitmap → membership change
  if (changes.smart_folder_ids?.length && scope.kind === 'smart_folder') {
    if (scope.id != null && changes.smart_folder_ids.includes(scope.id)) {
      return 'reconcile_membership';
    }
    return 'ignore';
  }

  // 6. Tag changes → membership change for untagged
  if (changes.tags_changed && scope.kind === 'system' && scope.key === 'untagged') {
    return 'reconcile_membership';
  }

  // 7. Derivative-only without entity hashes — can't check visibility
  if (changes.media_derivatives_changed && !changes.status_changed
      && !changes.folder_membership_changed?.length) {
    return 'ignore';
  }

  // 8. Compiler batch — sidebar-only work (rename, reorder, color).
  // Smart folder content rebuilds always carry extra_grid_scopes (handled by step 1).
  if (changes.compiler_batch_done) {
    return 'ignore';
  }

  // 9. Entity hashes with membership signals on system scope
  if (changes.entity_hashes?.length && scope.kind === 'system') {
    return 'reconcile_membership';
  }

  return 'ignore';
}

export function startGridSettle() {
  listen<{ changes: StateChanges }>('runtime/state_changed', (event) => {
    if (!store.get(gridActiveAtom)) return;

    const scope = store.get(gridScopeAtom);
    const action = classifyGridAction(event.payload.changes, scope);

    switch (action) {
      case 'ignore':
        break;
      case 'reconcile_metadata':
        gridController.reconcile(true);  // Safe to patch — metadata only
        break;
      case 'reconcile_membership':
        gridController.reconcile(false); // Membership may have changed — backend decides
        break;
      case 'refresh':
        gridController.loadFirstPage({ preserveItems: true });
        break;
    }
  });
}
