import type { StateChangedEvent, StateChanges, ResourceKey } from '../../shared/types/backendState';

/**
 * Plan the set of refresh targets implied by a state change.
 * Operates entirely from `event.changes`.
 * Pure function — no side effects, fully testable.
 */
export function planRefreshTargets(event: StateChangedEvent): Set<ResourceKey> {
  const keys = new Set<ResourceKey>();
  const changes = event.changes;
  const scopes: string[] = [];
  const hasTagMembershipChange = changes.tags_changed || Boolean(changes.tag_changes);
  const hasMediaFieldChange = changes.media_metadata_changed || Boolean(changes.media_fields_changed?.length);
  const hasDerivativeFieldChange = changes.media_derivatives_changed || Boolean(changes.derivative_fields_changed?.length);

  // --- Fact-driven rules ---

  if (changes.status_changed) {
    keys.add('sidebar/tree');
    keys.add('selection/current');
    scopes.push(
      'system:active',
      'system:inbox',
      'system:trash',
      'system:untagged',
      'system:uncategorized',
      'smart:all',
    );
    if (changes.folder_ids) {
      for (const id of changes.folder_ids) {
        scopes.push(`folder:${id}`);
      }
    }
  }

  if (hasTagMembershipChange) {
    keys.add('selection/current');
    scopes.push('system:untagged');
    if (!changes.entity_hashes) {
      scopes.push('system:active');
    }
  }

  if (changes.tag_structure_changed) {
    keys.add('sidebar/tree');
    keys.add('selection/current');
    scopes.push('system:active', 'smart:all');
  }

  if (changes.folder_membership_changed) {
    keys.add('sidebar/tree');
    keys.add('selection/current');
    scopes.push('system:uncategorized');
    for (const id of changes.folder_membership_changed) {
      scopes.push(`folder:${id}`);
    }
  }

  if (changes.view_prefs_changed) {
    keys.add('view-prefs/current');
  }

  if (hasMediaFieldChange || hasDerivativeFieldChange) {
    keys.add('selection/current');
  }

  // --- Domain-driven rules (fallback for patterns without fact flags) ---

  if (!keys.has('sidebar/tree') && hasDomain(changes, 'sidebar')) {
    keys.add('sidebar/tree');
  }

  if (!keys.has('selection/current') && hasDomain(changes, 'selection')) {
    keys.add('selection/current');
  }

  if (hasDomain(changes, 'subscriptions')) {
    keys.add('subscriptions/list');
  }

  // compiler_batch_done refreshes sidebar tree only if Domain::Sidebar is present
  // (handled by the domain-driven rule above).
  if (changes.compiler_batch_done) {
    keys.add('sidebar/tree');
  }

  // --- Entity-reference rules ---

  if (changes.entity_hashes) {
    for (const hash of changes.entity_hashes) {
      keys.add(`metadata/hash:${hash}`);
    }
  }

  if (changes.member_hashes) {
    for (const hash of changes.member_hashes) {
      keys.add(`metadata/hash:${hash}`);
    }
  }

  // Folder IDs without folder_membership_changed → grid refresh for those
  // folder scopes only (e.g., reorder within a folder).
  if (!changes.folder_membership_changed && changes.folder_ids) {
    for (const id of changes.folder_ids) {
      scopes.push(`folder:${id}`);
    }
  }

  if (changes.smart_folder_ids) {
    keys.add('selection/current');
    for (const id of changes.smart_folder_ids) {
      scopes.push(`smart:${id}`);
    }
  }

  // --- Extra grid scopes (non-derivable from other facts) ---

  if (changes.extra_grid_scopes) {
    scopes.push(...changes.extra_grid_scopes);
  }

  // --- Sidebar counts ---

  if (event.sidebar_counts) {
    keys.add('sidebar/counts');
  }

  // --- Build grid resource keys from collected scopes ---

  for (const scope of scopes) {
    keys.add(`grid/${scope}`);
  }

  return keys;
}

function hasDomain(changes: StateChanges, domain: string): boolean {
  return changes.domains.includes(domain as StateChanges['domains'][number]);
}

/**
 * Check whether a grid refresh target matches the currently active scope.
 */
export function refreshTargetMatchesGridScope(
  resourceKey: ResourceKey,
  activeScope: string | null,
): boolean {
  if (!resourceKey.startsWith('grid/')) return false;
  const scope = resourceKey.slice('grid/'.length);

  if (!activeScope) return true;
  if (scope === activeScope) return true;
  if (activeScope.startsWith('folder:') && scope === 'folder:all') return true;
  if (activeScope.startsWith('smart:') && scope === 'smart:all') return true;
  if (activeScope.startsWith('system:') && scope === 'system:active') return true;

  return false;
}
