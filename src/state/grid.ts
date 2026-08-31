/** Canonical grid session state and read-only selectors. */

import { atom } from 'jotai';
import { selectAtom } from 'jotai/utils';
import type {
  BaseScope,
  CanonicalEntityGridItem,
  EntityViewQuery,
  ItemSort,
  SortDirection,
  SortField,
} from '../shared/types/canonical';
import type { GridSpacing, GridViewMode } from '../shared/types/grid';
import { activeNodeIdAtom, displayedSurfaceNodeIdAtom } from './navigation';
import { sidebarNodesAtom, folderNodesAtom } from './sidebar';
import { nodeIdToGridScope } from '../shared/lib/gridScope';
import { compileGridQuery, createEmptyItemFilters, type ItemFilters } from '../shared/lib/itemFilters';
import { t } from '../i18n';

export type QueryFilters = ItemFilters;
export type { BaseScope, CanonicalEntityGridItem, SortDirection, SortField };

export interface GridViewPreferences {
  mode: GridViewMode;
  targetSize: number;
  /** Null inherits the application-wide spacing default. */
  spacing: GridSpacing | null;
  showName: boolean;
  showExtension: boolean;
  showExtensionLabel: boolean;
  showItemCount: boolean;
  showResolution: boolean;
  fitThumbnails: boolean;
  showSubfolders: boolean;
}

export interface GridSessionSnapshot {
  scope: BaseScope;
  sort: { field: SortField; direction: SortDirection; randomSeed?: string | null };
  searchText: string;
  filters: QueryFilters;
  view: GridViewPreferences;
  items: CanonicalEntityGridItem[];
  /** Opaque cursor for the next canonical page, or null at the end. */
  cursor: string | null;
  totalCount: number | null;
  totalSizeBytes: number | null;
  /** Canonical database/projection revision used to produce the loaded window. */
  revision: number;
  status: 'idle' | 'loading' | 'appending' | 'error';
  error: string | null;
  generation: number;
  active: boolean;
}

export type GridIntent =
  | { type: 'filter'; filters: QueryFilters; restoreScroll?: boolean }
  | { type: 'sort'; field: SortField; direction: SortDirection }
  | { type: 'view'; patch: Partial<GridViewPreferences> };

export const initialGridView: GridViewPreferences = {
  mode: 'waterfall',
  targetSize: 220,
  spacing: null,
  showName: true,
  showExtension: false,
  showExtensionLabel: false,
  showItemCount: true,
  showResolution: false,
  fitThumbnails: false,
  showSubfolders: true,
};

export const initialGridFilters: QueryFilters = createEmptyItemFilters();

export const gridSessionAtom = atom<GridSessionSnapshot>({
  scope: { kind: 'all' },
  sort: { field: 'imported_at', direction: 'descending' },
  searchText: '',
  filters: initialGridFilters,
  view: initialGridView,
  items: [],
  cursor: null,
  totalCount: null,
  totalSizeBytes: null,
  revision: 0,
  status: 'idle',
  error: null,
  generation: 0,
  active: true,
});

export const pendingGridIntentAtom = atom<GridIntent | null>(null);
export const pendingGridNavigationAtom = atom<{
  nodeId: string;
  filters: QueryFilters;
  sort?: ItemSort;
  restoreScroll?: boolean;
} | null>(null);
/** A grid drill-down rendered inside a manager while its sidebar node stays active. */
export const gridDrilldownAtom = atom<{
  ownerNodeId: string;
  scopeNodeId: string;
  filters: QueryFilters;
} | null>(null);
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
export const gridShowItemCountAtom = pick((s) => s.view.showItemCount);
export const gridShowResolutionAtom = pick((s) => s.view.showResolution);
export const gridFitThumbnailsAtom = pick((s) => s.view.fitThumbnails);
/** Transient grid display mode. It is deliberately not a per-scope library preference. */
export const gridGrayscaleAtom = atom(false);
/** Keep the current filters when navigating directly between grid scopes. */
export const gridFilterLockedAtom = atom(false);
/** Settings owns the fallback; an explicit per-scope view preference wins. */
export const gridDefaultSpacingAtom = atom<GridSpacing>('wide');
export const gridSpacingAtom = atom((get) => (
  get(gridSessionAtom).view.spacing ?? get(gridDefaultSpacingAtom)
));
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
  return compileGridQuery(
    session.scope,
    session.filters,
    {
      field: session.sort.field,
      direction: session.sort.direction,
      random_seed: session.sort.randomSeed ?? null,
    },
    session.searchText,
    session.view.showSubfolders,
  );
});

export const activeGridScopeAtom = atom((get) => nodeIdToGridScope(get(activeNodeIdAtom)));
export const gridVisibleItemIdsAtom = atom((get) => get(gridSessionAtom).items.map((item) => item.root_id));
export const gridReconcileContextAtom = atom((get) => ({
  scope: get(gridSessionAtom).scope,
  query: get(currentGridQueryAtom),
  visibleItemIds: get(gridVisibleItemIdsAtom),
}));

export type GridTransitionPhase = 'idle' | 'fading_out' | 'waiting' | 'fading_in';
export const gridTransitionPhaseAtom = atom<GridTransitionPhase>('idle');
/** Visibility of persistent filter chips below the titlebar. */
export const gridFilterToolbarOpenAtom = atom(false);

export const gridScopeLabelAtom = atom((get) => {
  if (get(gridSessionAtom).scope.kind === 'media_matches') return t('Items with This Media');
  const nodeId = get(displayedSurfaceNodeIdAtom);
  const labels: Record<string, string> = {
    'system:active': t('All'),
    'system:inbox': t('Inbox'),
    'system:trash': t('Trash'),
    'system:uncategorized': t('Uncategorized'),
    'system:untagged': t('Untagged'),
    'system:recent_viewed': t('Recently Viewed'),
    'system:random': t('Random'),
  };
  if (labels[nodeId]) return labels[nodeId];
  return get(sidebarNodesAtom).find((node) => node.id === nodeId)?.name ?? '';
});

export const gridChildFoldersAtom = atom((get) => {
  const session = get(gridSessionAtom);
  const scope = session.scope;
  if (scope.kind !== 'folder') return [];
  const nodes = get(folderNodesAtom);
  const children = new Map<string, typeof nodes>();
  for (const node of nodes) {
    if (!node.parent_id) continue;
    const siblings = children.get(node.parent_id) ?? [];
    siblings.push(node);
    children.set(node.parent_id, siblings);
  }
  for (const siblings of children.values()) {
    siblings.sort((a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0) || a.name.localeCompare(b.name));
  }
  const parentId = `folder:${scope.folder_id}`;
  if (!session.view.showSubfolders) return children.get(parentId) ?? [];
  const descendants: typeof nodes = [];
  const seen = new Set<string>();
  const append = (parentId: string) => {
    for (const child of children.get(parentId) ?? []) {
      if (seen.has(child.id)) continue;
      seen.add(child.id);
      descendants.push(child);
      append(child.id);
    }
  };
  append(parentId);
  return descendants;
});

export type { GridViewMode };
