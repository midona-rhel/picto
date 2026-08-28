import { getDefaultStore } from 'jotai';
import { activeNodeIdAtom } from '../state/navigation';
import { gridFiltersAtom, type QueryFilters } from '../state/grid';
import { navigateWithGridFilters } from '../state/navigationHistory';
import { createEmptyItemFilters } from '../shared/lib/itemFilters';
import type { TagFilterChoice } from '../shared/lib/itemFilters';
import { nodeIdToGridScope } from '../shared/lib/gridScope';

const store = getDefaultStore();

/** Open a grid scope and install its filters as one navigation operation. */
export function openGridWithFilters(nodeId: string, filters: QueryFilters): void {
  navigateWithGridFilters(nodeId, filters);
}

export function showTagItems(tag: TagFilterChoice): void {
  const currentNodeId = store.get(activeNodeIdAtom);
  const currentScope = nodeIdToGridScope(currentNodeId);
  const nodeId = currentScope ? currentNodeId : 'system:active';
  const filters = currentScope ? store.get(gridFiltersAtom) : createEmptyItemFilters();
  const nextFilters = {
    ...filters,
    include_tags: filters.include_tags.some((candidate) => candidate.tag_id === tag.tag_id)
      ? filters.include_tags
      : [...filters.include_tags, tag],
  };
  if (currentScope) openGridWithFilters(nodeId, nextFilters);
  else navigateWithGridFilters(nodeId, nextFilters, currentNodeId);
}

/** Open a filtered grid as a drill-down from Tag Manager. */
export function showTagManagerItems(tag: TagFilterChoice): void {
  navigateWithGridFilters(
    'system:active',
    { ...createEmptyItemFilters(), include_tags: [tag] },
    'system:tag_manager',
  );
}
