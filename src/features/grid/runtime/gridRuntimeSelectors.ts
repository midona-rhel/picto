import type { GridRuntimeState } from './gridRuntimeState';
import type { MasonryImageItem } from '../shared';
import type { SelectionQuerySpec } from '../metadataPrefetch';

/**
 * Returns the effective set of selected hashes, accounting for virtual-all mode.
 * In virtual-all mode, every visible image is selected except explicitly excluded ones.
 * In normal mode, returns the raw selectedHashes set.
 */
export function effectiveSelectedHashes(state: GridRuntimeState): Set<string> {
  if (!state.virtualAllSelection) return state.selectedHashes;
  const next = new Set<string>();
  for (const img of state.images) {
    if (!state.virtualAllSelection.excludedHashes.has(img.hash)) next.add(img.hash);
  }
  return next;
}

/**
 * Returns the single selected hash if exactly one is selected, otherwise null.
 * Useful for the inspector panel.
 */
export function singleSelectedHash(state: GridRuntimeState): string | null {
  const effective = effectiveSelectedHashes(state);
  if (effective.size === 1) return effective.values().next().value!;
  return null;
}

/**
 * Returns the selected images for the inspector.
 * In virtual-all mode, returns all loaded images matching the selection.
 * In normal mode, returns all explicitly selected images.
 */
export function selectedImagesPreview(state: GridRuntimeState): MasonryImageItem[] {
  if (state.virtualAllSelection) {
    return state.images
      .filter(i => !state.virtualAllSelection!.excludedHashes.has(i.hash));
  }
  return state.images.filter(i => state.selectedHashes.has(i.hash));
}

/**
 * Builds a SelectionQuerySpec from the virtual-all selection state.
 * Returns null when not in virtual-all mode.
 */
export function virtualSelectionSpec(state: GridRuntimeState): SelectionQuerySpec | null {
  if (!state.virtualAllSelection) return null;
  return {
    ...state.virtualAllSelection.baseSpec,
    excluded_hashes: [...state.virtualAllSelection.excludedHashes],
  };
}

/**
 * Builds an explicit-hashes SelectionQuerySpec from the given hash array.
 */
export function buildExplicitSelectionSpec(hashes: string[]): SelectionQuerySpec {
  return {
    mode: 'explicit_hashes',
    hashes,
    scope: {
      kind: 'system',
      system_key: 'all',
    },
    filters: {},
    sort: {},
    excluded_hashes: null,
    included_hashes: null,
  };
}

/**
 * Whether the grid should be frozen (transition in progress or externally frozen).
 */
export function isGridFrozen(_state: GridRuntimeState, externalFreeze: boolean): boolean {
  return externalFreeze;
}
