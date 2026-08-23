import type { EntityTarget } from '../../shared/types/canonical';

/** Capture the selection represented by a context menu when it opens. */
export function resolveContextMenuTarget(
  querySelectionActive: boolean,
  currentTarget: EntityTarget | null,
  visibleHashes: ReadonlySet<string>,
): EntityTarget | null {
  if (querySelectionActive && currentTarget?.kind === 'query_results') {
    return currentTarget;
  }
  if (visibleHashes.size === 0) return null;
  return { kind: 'entity_hashes', entity_hashes: [...visibleHashes] };
}
