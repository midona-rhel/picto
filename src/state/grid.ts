/**
 * Grid state — visible items, pagination, loading, sort, view mode, and display options.
 */

import { atom } from 'jotai';
import type { ItemQuery } from '../shared/types/generated/application/ItemQuery';
import type { ItemFilters } from '../shared/types/generated/application/ItemFilters';
import type { ItemScope } from '../shared/types/generated/application/ItemScope';
import type { ItemSortField } from '../shared/types/generated/application/ItemSortField';
import type { ItemSummary } from '../shared/types/generated/application/ItemSummary';
import type { SortDirection as BackendSortDirection } from '../shared/types/generated/application/SortDirection';
import type { GridViewMode } from '../shared/types/grid';
import { activeNodeIdAtom } from './navigation';
import { sidebarNodesAtom, folderNodesAtom } from './sidebar';
import { nodeIdToGridScope } from '../shared/lib/gridScope';

// ── Query inputs ─────────────────────────────────────────────────

export const gridScopeAtom = atom<ItemScope>({ kind: 'all' });
export const activeGridScopeAtom = atom((get) => nodeIdToGridScope(get(activeNodeIdAtom)));

export type SortField = ItemSortField;
export type SortDirection = BackendSortDirection;

export const gridSortFieldAtom = atom<SortField>('imported_at');
export const gridSortDirectionAtom = atom<SortDirection>('descending');
export const gridSearchTextAtom = atom<string>('');
export const gridFiltersAtom = atom<ItemFilters>({
  include_tags: [],
  exclude_tags: [],
  minimum_rating: null,
  mime_prefix: null,
  text: null,
});

export const currentGridQueryAtom = atom<ItemQuery>((get) => {
  const searchText = get(gridSearchTextAtom).trim();
  const filters = get(gridFiltersAtom);
  return {
    scope: get(gridScopeAtom),
    filters: {
      ...filters,
      text: searchText || null,
    },
    sort: {
      field: get(gridSortFieldAtom),
      direction: get(gridSortDirectionAtom),
      random_seed: null,
    },
  };
});

// ── View options ─────────────────────────────────────────────────

export const gridViewModeAtom = atom<GridViewMode>('waterfall');
export const gridTargetSizeAtom = atom(220);
export const gridShowNameAtom = atom(true);
export const gridShowExtensionAtom = atom(false);
export const gridShowResolutionAtom = atom(false);
export const gridShowExtensionLabelAtom = atom(false);
export const gridFitThumbnailsAtom = atom(false);
export const gridShowSubfoldersAtom = atom(true);

// ── Results ──────────────────────────────────────────────────────

export const gridItemsAtom = atom<ItemSummary[]>([]);
export const gridCursorAtom = atom<number | null>(null);
export const gridTotalCountAtom = atom<number | null>(null);
export const gridTotalSizeBytesAtom = atom<number | null>(null);
export const gridLoadingAtom = atom(false);
export const gridErrorAtom = atom<string | null>(null);
export const gridVisibleItemIdsAtom = atom((get) => get(gridItemsAtom).map((item) => item.item_id));
export const gridReconcileContextAtom = atom((get) => ({
  scope: get(gridScopeAtom),
  query: get(currentGridQueryAtom),
  visibleItemIds: get(gridVisibleItemIdsAtom),
}));

/** Whether the grid is the active surface. */
export const gridActiveAtom = atom(true);

/** Grid transition phase — inspector/toolbar freeze when not 'idle'. */
export type GridTransitionPhase = 'idle' | 'fading_out' | 'waiting' | 'fading_in';
export const gridTransitionPhaseAtom = atom<GridTransitionPhase>('idle');

/** Whether grid-only chrome is entering or leaving a manager surface. */
export type GridChromeTransition = 'stable' | 'leaving_grid' | 'entering_grid';
export const gridChromeTransitionAtom = atom<GridChromeTransition>('stable');

/** Pending action to execute at the midpoint of a soft fade transition. */
export const gridSoftTransitionActionAtom = atom<(() => void) | null>(null);

/** Human-readable label for the current scope. Derived from sidebar nodes. */
export const gridScopeLabelAtom = atom((get) => {
  const nodeId = get(activeNodeIdAtom);
  const SYSTEM_LABELS: Record<string, string> = {
    'system:active': 'All',
    'system:inbox': 'Inbox',
    'system:trash': 'Trash',
    'system:uncategorized': 'Uncategorized',
    'system:untagged': 'Untagged',
  };
  // System scopes: always use our label, not the backend's name
  if (SYSTEM_LABELS[nodeId]) return SYSTEM_LABELS[nodeId];
  const nodes = get(sidebarNodesAtom);
  const node = nodes.find((n) => n.id === nodeId);
  if (node) return node.name;
  const fallbacks = SYSTEM_LABELS;
  return fallbacks[nodeId] ?? '';
});

// ── Child folders for subfolder grid tiles ──────────────────────

export const gridChildFoldersAtom = atom((get) => {
  const scope = get(gridScopeAtom);
  if (scope.kind !== 'folder') return [];
  const parentId = `folder:${scope.folder_id}`;
  return get(folderNodesAtom)
    .filter((n) => n.parent_id === parentId)
    .sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0) || a.name.localeCompare(b.name));
});

// Re-export types for convenience
export type { GridViewMode };
