import type { MutationReceipt, ResourceKey } from '../shared/types/generated/runtime-contract';

/**
 * Derive the set of resource keys that a MutationReceipt invalidates.
 * Pure function — no side effects, fully testable.
 */
export function deriveStaleResources(receipt: MutationReceipt): Set<ResourceKey> {
  const keys = new Set<ResourceKey>();
  const inv = receipt.invalidate;

  if (inv.sidebar_tree || receipt.facts.compiler_batch_done) {
    keys.add('sidebar/tree');
  }

  if (receipt.sidebar_counts) {
    keys.add('sidebar/counts');
  }

  if (inv.grid_scopes) {
    for (const scope of inv.grid_scopes) {
      keys.add(`grid/${scope}`);
    }
  }

  if (inv.metadata_hashes) {
    for (const hash of inv.metadata_hashes) {
      keys.add(`metadata/hash:${hash}`);
    }
  }

  if (inv.selection_summary) {
    keys.add('selection/current');
  }

  if (inv.view_prefs) {
    keys.add('view-prefs/current');
  }

  return keys;
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
