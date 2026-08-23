import type { ItemTarget } from '../../shared/types/generated/application/ItemTarget';

/** Capture the selection represented by a context menu when it opens. */
export function resolveContextMenuTarget(
  querySelectionActive: boolean,
  currentTarget: ItemTarget | null,
  visibleItemIds: ReadonlySet<number>,
): ItemTarget | null {
  if (querySelectionActive && currentTarget?.kind === 'query') {
    return currentTarget;
  }
  if (visibleItemIds.size === 0) return null;
  return { kind: 'explicit', item_ids: [...visibleItemIds] };
}
