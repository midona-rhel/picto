/** Canonical grid session state and read-only selectors. */

import { atom } from 'jotai';
import { selectAtom } from 'jotai/utils';
import type { CanonicalEntityGridItem, BaseScope, EntityViewQuery, QueryFilters } from '../shared/types/canonical';
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
  scope: BaseScope;
  sort: { field: SortField; direction: SortDirection };
  searchText: string;
  filters: QueryFilters;
  view: GridViewPreferences;
  items: CanonicalEntityGridItem[];
  cursor: string | null;
  totalCount: number | null;
  totalSizeBytes: number | null;
  status: 'idle' | 'loading' | 'appending' | 'error';
  error: string | null;
  generation: number;
  active: boolean;
}

export type GridIntent =
  | { type: 'filter'; filters: QueryFilters }
  | { type: 'sort'; field: SortField; direction: SortDirection }
  | { type: 'view'; patch: Partial<GridViewPreferences> };

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

export const gridSessionAtom = atom<GridSessionSnapshot>({
  scope: { kind: 'system', key: 'all' },
  sort: { field: 'date_added', direction: 'desc' },
  searchText: '',
  filters: {},
  view: initialGridView,
  items: [],
  cursor: null,
  totalCount: null,
  totalSizeBytes: null,
  status: 'idle',
  error: null,
  generation: 0,
  active: true,
});

export const pendingGridIntentAtom = atom<GridIntent | null>(null);
const pick = <T>(selector: (session: GridSessionSnapshot) => T) => selectAtom(gridSessionAtom, selector);

export const gridScopeAtom = pick((s) => s.scope);
export const gridSortFieldAtom = pick((s) => s.sort.field);
export const gridSortDirectionAtom = pick((s) => s.sort.direction);
export const gridSearchTextAtom = pick((s) => s.searchText);
export const gridFiltersAtom = pick((s) => s.filters);
export const gridViewModeAtom = pick((s) => s.view.mode);
export const gridTargetSizeAtom = pick((s) => s.view.targetSize);
export const gridShowNameAtom = pick((s) => s.view.showName);
export const gridShowExtensionAtom = pick((s) => s.view.showExtension);
export const gridShowExtensionLabelAtom = pick((s) => s.view.showExtensionLabel);
export const gridShowResolutionAtom = pick((s) => s.view.showResolution);
export const gridFitThumbnailsAtom = pick((s) => s.view.fitThumbnails);
export const gridShowSubfoldersAtom = pick((s) => s.view.showSubfolders);
export const gridItemsAtom = pick((s) => s.items);
export const gridCursorAtom = pick((s) => s.cursor);
export const gridTotalCountAtom = pick((s) => s.totalCount);
export const gridTotalSizeBytesAtom = pick((s) => s.totalSizeBytes);
export const gridLoadingAtom = pick((s) => s.status === 'loading');
export const gridErrorAtom = pick((s) => s.error);
export const gridActiveAtom = pick((s) => s.active);

export const currentGridQueryAtom = atom<EntityViewQuery>((get) => {
  const session = get(gridSessionAtom);
  const searchText = session.searchText.trim();
  const hasFilters = Object.values(session.filters)
    .some((value) => value != null && (!Array.isArray(value) || value.length > 0));
  return {
    base_scope: session.scope,
    filters: searchText || hasFilters ? { ...session.filters, search_text: searchText || undefined } : undefined,
    sort: session.sort,
  };
});

export const activeGridScopeAtom = atom((get) => nodeIdToGridScope(get(activeNodeIdAtom)));
export const gridVisibleEntityHashesAtom = atom((get) => get(gridSessionAtom).items.map((item) => item.entity_hash));
export const gridReconcileContextAtom = atom((get) => ({
  scope: get(gridSessionAtom).scope,
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
  const scope = get(gridSessionAtom).scope;
  if (scope.kind !== 'folder' || scope.id == null) return [];
  const parentId = `folder:${scope.id}`;
  return get(folderNodesAtom)
    .filter((node) => node.parent_id === parentId)
    .sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0) || a.name.localeCompare(b.name));
});

export type { GridViewMode };
