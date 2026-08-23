/** Canonical grid session state and read-only selectors. */

import { atom } from 'jotai';
import { selectAtom } from 'jotai/utils';
import type { BaseScope, EntityViewPage, EntityViewQuery, QueryFilters } from '../shared/types/canonical';
import type { GridViewMode } from '../shared/types/grid';
import { activeNodeIdAtom } from './navigation';
import { sidebarNodesAtom, folderNodesAtom } from './sidebar';
import { nodeIdToGridScope } from '../shared/lib/gridScope';

export type SortField = 'date_added' | 'date_created' | 'date_modified' | 'rating' | 'name' | 'size_bytes' | 'duration';
export type SortDirection = 'asc' | 'desc';

export interface GridViewPreferences {
  mode: GridViewMode;
  targetSize: number;
  showName: boolean;
  showExtension: boolean;
  showExtensionLabel: boolean;
  showResolution: boolean;
  fitThumbnails: boolean;
  showSubfolders: boolean;
}

export interface GridSessionSnapshot {
  query: EntityViewQuery;
  view: GridViewPreferences;
  pages: EntityViewPage[];
  status: 'idle' | 'loading' | 'appending' | 'error';
  error: string | null;
  generation: number;
  active: boolean;
}

export type GridImpact = 'metadata' | 'membership' | 'order' | 'reload';
export type GridIntent =
  | { type: 'navigate'; scope: BaseScope }
  | { type: 'search'; text: string }
  | { type: 'filter'; filters: QueryFilters }
  | { type: 'sort'; field: SortField; direction: SortDirection }
  | { type: 'view'; patch: Partial<GridViewPreferences>; transition?: boolean }
  | { type: 'load_next' }
  | { type: 'reconcile'; impact: GridImpact };

export const initialGridView: GridViewPreferences = {
  mode: 'waterfall',
  targetSize: 220,
  showName: true,
  showExtension: false,
  showExtensionLabel: false,
  showResolution: false,
  fitThumbnails: false,
  showSubfolders: true,
};

export function reduceGridSession(session: GridSessionSnapshot, intent: GridIntent): GridSessionSnapshot {
  switch (intent.type) {
    case 'navigate':
      return { ...session, query: { base_scope: intent.scope, sort: session.query.sort } };
    case 'search':
      return { ...session, query: {
        ...session.query,
        filters: { ...session.query.filters, search_text: intent.text || undefined },
      } };
    case 'filter':
      return { ...session, query: {
        ...session.query,
        filters: { ...intent.filters, search_text: session.query.filters?.search_text },
      } };
    case 'sort':
      return { ...session, query: {
        ...session.query,
        sort: { field: intent.field, direction: intent.direction },
      } };
    case 'view':
      return { ...session, view: { ...session.view, ...intent.patch } };
    default:
      return session;
  }
}

export const gridSessionAtom = atom<GridSessionSnapshot>({
  query: {
    base_scope: { kind: 'system', key: 'all' },
    sort: { field: 'date_added', direction: 'desc' },
  },
  view: initialGridView,
  pages: [],
  status: 'idle',
  error: null,
  generation: 0,
  active: true,
});

export const pendingGridIntentAtom = atom<GridIntent | null>(null);
const pick = <T>(selector: (session: GridSessionSnapshot) => T) => selectAtom(gridSessionAtom, selector);
const EMPTY_FILTERS: QueryFilters = {};
const gridPagesAtom = pick((s) => s.pages);

export const gridScopeAtom = pick((s) => s.query.base_scope);
export const gridSortFieldAtom = pick((s) => (s.query.sort?.field as SortField) ?? 'date_added');
export const gridSortDirectionAtom = pick((s) => (s.query.sort?.direction as SortDirection) ?? 'desc');
export const gridSearchTextAtom = pick((s) => s.query.filters?.search_text ?? '');
export const gridFiltersAtom = pick((s) => s.query.filters ?? EMPTY_FILTERS);
export const gridViewAtom = pick((s) => s.view);
export const gridViewModeAtom = pick((s) => s.view.mode);
export const gridTargetSizeAtom = pick((s) => s.view.targetSize);
export const gridShowNameAtom = pick((s) => s.view.showName);
export const gridShowExtensionAtom = pick((s) => s.view.showExtension);
export const gridShowExtensionLabelAtom = pick((s) => s.view.showExtensionLabel);
export const gridShowResolutionAtom = pick((s) => s.view.showResolution);
export const gridFitThumbnailsAtom = pick((s) => s.view.fitThumbnails);
export const gridShowSubfoldersAtom = pick((s) => s.view.showSubfolders);
export const gridItemsAtom = atom((get) => get(gridPagesAtom).flatMap((page) => page.items));
export const gridCursorAtom = atom((get) => {
  const pages = get(gridPagesAtom);
  return pages[pages.length - 1]?.next_cursor ?? null;
});
export const gridTotalCountAtom = atom((get) => get(gridPagesAtom)[0]?.total_count ?? null);
export const gridTotalSizeBytesAtom = atom((get) => get(gridPagesAtom)[0]?.total_size_bytes ?? null);
export const gridLoadingAtom = pick((s) => s.status === 'loading');
export const gridErrorAtom = pick((s) => s.error);
export const gridActiveAtom = pick((s) => s.active);

export const currentGridQueryAtom = atom<EntityViewQuery>((get) => {
  const session = get(gridSessionAtom);
  return session.query;
});

export const activeGridScopeAtom = atom((get) => nodeIdToGridScope(get(activeNodeIdAtom)));
export const gridVisibleEntityHashesAtom = atom((get) => get(gridItemsAtom).map((item) => item.entity_hash));
export const gridReconcileContextAtom = atom((get) => ({
  scope: get(gridScopeAtom),
  query: get(currentGridQueryAtom),
  visibleEntityHashes: get(gridVisibleEntityHashesAtom),
}));

export type GridTransitionPhase = 'idle' | 'fading_out' | 'waiting' | 'fading_in';
export const gridTransitionPhaseAtom = atom<GridTransitionPhase>('idle');

export const gridScopeLabelAtom = atom((get) => {
  const nodeId = get(activeNodeIdAtom);
  const labels: Record<string, string> = {
    'system:active': 'All', 'system:inbox': 'Inbox', 'system:trash': 'Trash',
    'system:uncategorized': 'Uncategorized', 'system:untagged': 'Untagged',
  };
  if (labels[nodeId]) return labels[nodeId];
  return get(sidebarNodesAtom).find((node) => node.id === nodeId)?.name ?? '';
});

export const gridChildFoldersAtom = atom((get) => {
  const scope = get(gridScopeAtom);
  if (scope.kind !== 'folder' || scope.id == null) return [];
  const parentId = `folder:${scope.id}`;
  return get(folderNodesAtom)
    .filter((node) => node.parent_id === parentId)
    .sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0) || a.name.localeCompare(b.name));
});

export type { GridViewMode };
