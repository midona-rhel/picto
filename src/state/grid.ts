/**
 * Grid state — visible items, pagination, loading, sort, view mode, and display options.
 */

import { atom } from 'jotai';
import type { CanonicalEntityGridItem, BaseScope, EntityViewQuery } from '../shared/types/canonical';
import type { GridViewMode } from '../features/grid/layout/types';
import { activeNodeIdAtom, collectionNameAtom } from './navigation';
import { sidebarNodesAtom, folderNodesAtom } from './sidebar';
import { nodeIdToGridScope } from '../shared/lib/gridScope';

// ── Query inputs ─────────────────────────────────────────────────

export const gridScopeAtom = atom<BaseScope>({ kind: 'system', key: 'all' });
export const activeGridScopeAtom = atom((get) => nodeIdToGridScope(get(activeNodeIdAtom)));
export const activeGridNodeIdAtom = atom((get) => {
  const nodeId = get(activeNodeIdAtom);
  return nodeIdToGridScope(nodeId) ? nodeId : null;
});

export type SortField = 'date_added' | 'date_created' | 'date_modified' | 'rating' | 'name' | 'size_bytes' | 'duration';
export type SortDirection = 'asc' | 'desc';

export const gridSortFieldAtom = atom<SortField>('date_added');
export const gridSortDirectionAtom = atom<SortDirection>('desc');
export const gridSearchTextAtom = atom<string>('');

export const currentGridQueryAtom = atom<EntityViewQuery>((get) => {
  const searchText = get(gridSearchTextAtom).trim();
  return {
    base_scope: get(gridScopeAtom),
    filters: searchText ? { search_text: searchText } : undefined,
    sort: {
      field: get(gridSortFieldAtom),
      direction: get(gridSortDirectionAtom),
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
export const stripFitModeAtom = atom<'horizontal' | 'vertical'>('horizontal');
export const gridFitThumbnailsAtom = atom(false);
export const gridShowSubfoldersAtom = atom(true);

// ── Results ──────────────────────────────────────────────────────

export const gridItemsAtom = atom<CanonicalEntityGridItem[]>([]);
export const gridCursorAtom = atom<string | null>(null);
export const gridTotalCountAtom = atom<number | null>(null);
export const gridTotalSizeBytesAtom = atom<number | null>(null);
export const gridLoadingAtom = atom(false);
export const gridErrorAtom = atom<string | null>(null);
export const gridVisibleEntityHashesAtom = atom((get) => get(gridItemsAtom).map((item) => item.entity_hash));
export const gridReconcileContextAtom = atom((get) => ({
  scope: get(gridScopeAtom),
  query: get(currentGridQueryAtom),
  visibleEntityHashes: get(gridVisibleEntityHashesAtom),
}));

/** Whether the grid is the active surface. */
export const gridActiveAtom = atom(true);

/** Grid transition phase — inspector/toolbar freeze when not 'idle'. */
export type GridTransitionPhase = 'idle' | 'fading_out' | 'waiting' | 'fading_in';
export const gridTransitionPhaseAtom = atom<GridTransitionPhase>('idle');

/** Pending action to execute at the midpoint of a soft fade transition. */
export const gridSoftTransitionActionAtom = atom<(() => void) | null>(null);

// ── Derived ──────────────────────────────────────────────────────

export const gridEmptyAtom = atom((get) =>
  get(gridItemsAtom).length === 0 && !get(gridLoadingAtom),
);

/** Human-readable label for the current scope. Derived from sidebar nodes. */
export const gridScopeLabelAtom = atom((get) => {
  const nodeId = get(activeNodeIdAtom);
  // Collection scope — use the stored collection name
  if (nodeId.startsWith('collection:')) {
    return get(collectionNameAtom) ?? 'Collection';
  }
  const nodes = get(sidebarNodesAtom);
  const node = nodes.find((n) => n.id === nodeId);
  if (node) return node.name;
  const fallbacks: Record<string, string> = {
    'system:active': 'All',
    'system:inbox': 'Inbox',
    'system:trash': 'Trash',
    'system:uncategorized': 'Uncategorized',
    'system:untagged': 'Untagged',
  };
  return fallbacks[nodeId] ?? '';
});

// ── Child folders for subfolder grid tiles ──────────────────────

export const gridChildFoldersAtom = atom((get) => {
  const scope = get(gridScopeAtom);
  if (scope.kind !== 'folder' || scope.id == null) return [];
  const parentId = `folder:${scope.id}`;
  return get(folderNodesAtom)
    .filter((n) => n.parent_id === parentId)
    .sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0) || a.name.localeCompare(b.name));
});

// Re-export types for convenience
export type { GridViewMode };
