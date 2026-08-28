import type { EntityTarget } from '../../shared/types/canonical';

/** Capture the selection represented by a context menu when it opens. */
export function resolveContextMenuTarget(
  querySelectionActive: boolean,
  currentTarget: EntityTarget | null,
  visibleItemIds: ReadonlySet<number>,
): EntityTarget | null {
  if (querySelectionActive && currentTarget?.kind === 'query') {
    return currentTarget;
  }
  if (visibleItemIds.size === 0) return null;
  return { kind: 'explicit', root_ids: [...visibleItemIds] };
}
