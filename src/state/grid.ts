/**
 * Grid state — visible items, pagination, loading, sort, view mode, and display options.
 */

import { atom } from 'jotai';
import type { CanonicalEntityGridItem, BaseScope } from '../shared/types/canonical';
import type { GridViewMode } from '../features/grid/layout/types';
import { activeNodeIdAtom } from './navigation';
import { sidebarNodesAtom } from './sidebar';

// ── Query inputs ─────────────────────────────────────────────────

export const gridScopeAtom = atom<BaseScope>({ kind: 'system', key: 'all' });

export type SortField = 'date_added' | 'date_created' | 'date_modified' | 'rating' | 'name' | 'size_bytes';
export type SortDirection = 'asc' | 'desc';

export const gridSortFieldAtom = atom<SortField>('date_added');
export const gridSortDirectionAtom = atom<SortDirection>('desc');

// ── View options ─────────────────────────────────────────────────

export const gridViewModeAtom = atom<GridViewMode>('waterfall');
export const gridTargetSizeAtom = atom(220);
export const gridShowNameAtom = atom(true);
export const gridShowExtensionAtom = atom(false);

// ── Results ──────────────────────────────────────────────────────

export const gridItemsAtom = atom<CanonicalEntityGridItem[]>([]);
export const gridCursorAtom = atom<string | null>(null);
export const gridTotalCountAtom = atom<number | null>(null);
export const gridLoadingAtom = atom(false);
export const gridErrorAtom = atom<string | null>(null);

/** Whether the grid is the active surface. */
export const gridActiveAtom = atom(true);

// ── Derived ──────────────────────────────────────────────────────

export const gridEmptyAtom = atom((get) =>
  get(gridItemsAtom).length === 0 && !get(gridLoadingAtom),
);

/** Human-readable label for the current scope. Derived from sidebar nodes. */
export const gridScopeLabelAtom = atom((get) => {
  const nodeId = get(activeNodeIdAtom);
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

// Re-export types for convenience
export type { GridViewMode };
