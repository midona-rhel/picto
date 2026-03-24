import type { ResourceKey } from '../../shared/types/backendState';

export interface InspectorRefreshScope {
  selectedHashes: string[];
  hasVirtualSelection: boolean;
  hasSelectedCollection: boolean;
}

/**
 * Decide whether a batch of planned refresh targets should trigger an
 * inspector data refresh for the current files/media selection.
 */
export function inspectorNeedsRefresh(
  scope: InspectorRefreshScope,
  refreshTargets: Set<ResourceKey>,
): boolean {
  if (scope.hasSelectedCollection) return false;
  if (scope.hasVirtualSelection) {
    return refreshTargets.has('selection/current');
  }
  if (scope.selectedHashes.length === 0) return false;

  if (scope.selectedHashes.length > 1 && refreshTargets.has('selection/current')) {
    return true;
  }

  return scope.selectedHashes.some((hash) => refreshTargets.has(`metadata/hash:${hash}`));
}
