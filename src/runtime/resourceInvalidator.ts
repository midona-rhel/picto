import type { MutationReceipt, MutationFacts, ResourceKey } from '../shared/types/generated/runtime-contract';

/**
 * Derive the set of resource keys that a MutationReceipt invalidates.
 * Operates entirely from `receipt.facts` — does NOT read `receipt.invalidate`.
 * Pure function — no side effects, fully testable.
 */
export function deriveStaleResources(receipt: MutationReceipt): Set<ResourceKey> {
  const keys = new Set<ResourceKey>();
  const facts = receipt.facts;
  const scopes: string[] = [];

  // --- Fact-driven rules ---

  if (facts.status_changed) {
    keys.add('sidebar/tree');
    keys.add('selection/current');
    scopes.push('system:all', 'system:inbox', 'system:trash', 'system:recently_viewed', 'smart:all');
    if (facts.folder_ids) {
      for (const id of facts.folder_ids) {
        scopes.push(`folder:${id}`);
      }
    }
  }

  if (facts.tags_changed) {
    keys.add('selection/current');
    if (!facts.file_hashes) {
      scopes.push('system:all');
    }
  }

  if (facts.tag_structure_changed) {
    keys.add('sidebar/tree');
    keys.add('selection/current');
    scopes.push('system:all', 'smart:all');
  }

  if (facts.folder_membership_changed) {
    keys.add('sidebar/tree');
    keys.add('selection/current');
    for (const id of facts.folder_membership_changed) {
      scopes.push(`folder:${id}`);
    }
  }

  if (facts.view_prefs_changed) {
    keys.add('view-prefs/current');
  }

  // --- Domain-driven rules (fallback for patterns without fact flags) ---

  if (!keys.has('sidebar/tree') && hasDomain(facts, 'sidebar')) {
    keys.add('sidebar/tree');
  }

  if (!keys.has('selection/current') && hasDomain(facts, 'selection')) {
    keys.add('selection/current');
  }

  // compiler_batch_done refreshes sidebar tree only if Domain::Sidebar is present
  // (handled by the domain-driven rule above).
  if (facts.compiler_batch_done) {
    keys.add('sidebar/tree');
  }

  // --- Entity-reference rules ---

  if (facts.file_hashes) {
    for (const hash of facts.file_hashes) {
      keys.add(`metadata/hash:${hash}`);
    }
  }

  // Folder IDs without folder_membership_changed → grid refresh for those
  // folder scopes only (e.g., reorder within a folder).
  if (!facts.folder_membership_changed && facts.folder_ids) {
    for (const id of facts.folder_ids) {
      scopes.push(`folder:${id}`);
    }
  }

  if (facts.smart_folder_ids) {
    keys.add('selection/current');
    for (const id of facts.smart_folder_ids) {
      scopes.push(`smart:${id}`);
    }
  }

  // --- Extra grid scopes (non-derivable from other facts) ---

  if (facts.extra_grid_scopes) {
    scopes.push(...facts.extra_grid_scopes);
  }

  // --- Sidebar counts ---

  if (receipt.sidebar_counts) {
    keys.add('sidebar/counts');
  }

  // --- Build grid resource keys from collected scopes ---

  for (const scope of scopes) {
    keys.add(`grid/${scope}`);
  }

  return keys;
}

function hasDomain(facts: MutationFacts, domain: string): boolean {
  return facts.domains.includes(domain as MutationFacts['domains'][number]);
}

/**
 * Check whether a grid resource key matches the currently active scope.
 * Scope-aware grid invalidation filter for resource refresh.
 */
export function gridResourceMatchesScope(
  resourceKey: ResourceKey,
  activeScope: string | null,
): boolean {
  if (!resourceKey.startsWith('grid/')) return false;
  const scope = resourceKey.slice('grid/'.length);

  if (!activeScope) return true;
  if (scope === activeScope) return true;
  if (activeScope.startsWith('folder:') && scope === 'folder:all') return true;
  if (activeScope.startsWith('smart:') && scope === 'smart:all') return true;
  // system:all is a wildcard for non-specific scopes
  if (scope === 'system:all') return true;

  return false;
}
