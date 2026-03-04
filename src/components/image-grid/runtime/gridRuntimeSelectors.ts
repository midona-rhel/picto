import type { GridRuntimeState } from './gridRuntimeState';
import type { MasonryImageItem } from '../shared';
import type { SelectionQuerySpec } from '../metadataPrefetch';
import { isTransitionFrozen } from './gridTransitionPipeline';

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
 * Returns the selected images for preview/collage UI.
 * In virtual-all mode, returns up to 12 visible selected images.
 * In normal mode, returns all explicitly selected images.
 */
export function selectedImagesPreview(state: GridRuntimeState): MasonryImageItem[] {
  if (state.virtualAllSelection) {
    return state.images
      .filter(i => !state.virtualAllSelection!.excludedHashes.has(i.hash))
      .slice(0, 12);
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
    search_tags: null,
    search_excluded_tags: null,
    tag_match_mode: null,
    smart_folder_predicate: null,
    smart_folder_sort_field: null,
    smart_folder_sort_order: null,
    sort_field: null,
    sort_order: null,
    excluded_hashes: null,
    included_hashes: null,
    status: null,
    folder_ids: null,
    excluded_folder_ids: null,
    folder_match_mode: null,
  };
}

/**
 * Whether the grid should be frozen (transition in progress or externally frozen).
 */
export function isGridFrozen(state: GridRuntimeState, externalFreeze: boolean): boolean {
  return isTransitionFrozen(state.transitionStage) || externalFreeze;
}
