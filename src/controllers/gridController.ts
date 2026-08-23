/** Single command owner for the canonical grid session. */

import { getDefaultStore } from 'jotai';
import { queryItems } from '../platform/entityApi';
import { getViewPrefs, setViewPrefs } from '../platform/settingsApi';
import type { ViewPrefsDto, ViewPrefsPatch } from '../platform/settingsApi';
import { clearSelectionAtom } from '../state/selection';
import {
  currentGridQueryAtom,
  gridSessionAtom,
  initialGridFilters,
  initialGridView,
  pendingGridIntentAtom,
  type BaseScope,
  type GridIntent,
  type GridSessionSnapshot,
  type GridViewPreferences,
  type QueryFilters,
  type SortDirection,
  type SortField,
} from '../state/grid';

const store = getDefaultStore();
const PAGE_SIZE = 500;
const SEARCH_DEBOUNCE_MS = 300;
const VIEW_PREFS_SAVE_DEBOUNCE_MS = 500;

function scopeToKey(scope: BaseScope): string {
  switch (scope.kind) {
    case 'all': return 'system:active';
    case 'inbox': return 'system:inbox';
    case 'trash': return 'system:trash';
    case 'recently_viewed': return 'system:recent_viewed';
    case 'untagged': return 'system:untagged';
    case 'uncategorized': return 'system:uncategorized';
    case 'folder': return `folder:${scope.folder_id}`;
    case 'smart_folder': return `smart:${scope.smart_folder_id}`;
  }
}

function updateSession(
  update: Partial<GridSessionSnapshot> | ((current: GridSessionSnapshot) => GridSessionSnapshot),
): void {
  const current = store.get(gridSessionAtom);
  store.set(gridSessionAtom, typeof update === 'function' ? update(current) : { ...current, ...update });
}

function viewFromPreferences(
  scope: BaseScope,
  prefs: ViewPrefsDto | null,
  globals: ViewPrefsDto | null,
): GridViewPreferences {
  const value = (field: keyof ViewPrefsDto) => prefs?.[field] ?? globals?.[field] ?? null;
  return {
    mode: (value('view_mode') as GridViewPreferences['mode']) || initialGridView.mode,
    targetSize: (value('target_size') as number) ?? initialGridView.targetSize,
    showName: (value('show_name') as boolean) ?? initialGridView.showName,
    showResolution: (value('show_resolution') as boolean) ?? initialGridView.showResolution,
    showExtension: (value('show_extension') as boolean) ?? initialGridView.showExtension,
    showExtensionLabel: (value('show_label') as boolean) ?? initialGridView.showExtensionLabel,
    fitThumbnails: value('thumbnail_fit') === 'cover',
    showSubfolders: scope.kind === 'folder' ? initialGridView.showSubfolders : false,
  };
}

function preferenceSortField(value: string | null): SortField {
  switch (value) {
    case 'imported_at':
    case 'captured_at':
    case 'name':
    case 'rating':
    case 'size':
    case 'random':
    case 'folder_order':
      return value;
    default:
      return 'imported_at';
  }
}

function preferenceSortDirection(value: string | null): SortDirection {
  return value === 'ascending' ? 'ascending' : 'descending';
}

class GridSessionController {
  private searchTimer: ReturnType<typeof setTimeout> | null = null;
  private preferenceTimer: ReturnType<typeof setTimeout> | null = null;
  private pendingPreferencePatch: ViewPrefsPatch = {};
  private scopeKey = '';

  async navigateTo(scope: BaseScope): Promise<void> {
    this.cancelSearch();
    store.set(clearSelectionAtom);
    const key = scopeToKey(scope);
    this.scopeKey = key;
    const generation = store.get(gridSessionAtom).generation + 1;
    updateSession({
      scope,
      searchText: '',
      filters: { ...initialGridFilters },
      active: true,
      generation,
      status: 'loading',
      error: null,
      items: [],
      cursor: null,
      totalCount: null,
      totalSizeBytes: null,
    });

    const [prefs, globals] = await Promise.all([
      getViewPrefs(key).catch(() => null),
      getViewPrefs('').catch(() => null),
    ]);
    if (store.get(gridSessionAtom).generation !== generation) return;
    const sortField = prefs?.sort_field ?? globals?.sort_field ?? null;
    const sortOrder = prefs?.sort_order ?? globals?.sort_order ?? null;
    updateSession((current) => ({
      ...current,
      sort: {
        field: preferenceSortField(sortField),
        direction: preferenceSortDirection(sortOrder),
      },
      view: viewFromPreferences(scope, prefs, globals),
    }));
    await this.loadFirstPage({ generation });
  }

  deactivate(): void {
    this.cancelSearch();
    store.set(clearSelectionAtom);
    updateSession((current) => ({
      ...current,
      active: false,
      generation: current.generation + 1,
      status: 'idle',
    }));
  }

  setSearchText(text: string): void {
    updateSession({ searchText: text });
    this.cancelSearch();
    this.searchTimer = setTimeout(() => {
      this.searchTimer = null;
      void this.loadFirstPage({ preserveItems: true });
    }, SEARCH_DEBOUNCE_MS);
  }

  requestIntent(intent: GridIntent): void {
    store.set(pendingGridIntentAtom, intent);
  }

  applyIntent(intent: GridIntent): void {
    if (intent.type === 'filter') {
      this.setFiltersNow(intent.filters);
      return;
    }
    if (intent.type === 'sort') {
      this.setSortNow(intent.field, intent.direction);
      return;
    }
    this.updateView(intent.patch);
  }

  setFilters(filters: QueryFilters): void {
    this.requestIntent({ type: 'filter', filters });
  }

  setSort(field: SortField, direction: SortDirection): void {
    this.requestIntent({ type: 'sort', field, direction });
  }

  updateView(patch: Partial<GridViewPreferences>, transition = false): void {
    if (transition) {
      this.requestIntent({ type: 'view', patch });
      return;
    }
    updateSession((current) => ({ ...current, view: { ...current.view, ...patch } }));
  }

  saveViewPref(patch: ViewPrefsPatch): void {
    if (!this.scopeKey) return;
    this.pendingPreferencePatch = { ...this.pendingPreferencePatch, ...patch };
    if (this.preferenceTimer) clearTimeout(this.preferenceTimer);
    this.preferenceTimer = setTimeout(() => {
      this.preferenceTimer = null;
      const pending = this.pendingPreferencePatch;
      this.pendingPreferencePatch = {};
      void setViewPrefs(this.scopeKey, pending).catch(() => {});
    }, VIEW_PREFS_SAVE_DEBOUNCE_MS);
  }

  async loadFirstPage(options?: { preserveItems?: boolean; generation?: number }): Promise<void> {
    const generation = options?.generation ?? store.get(gridSessionAtom).generation + 1;
    updateSession((current) => ({
      ...current,
      generation,
      status: 'loading',
      error: null,
      items: options?.preserveItems ? current.items : [],
      cursor: null,
      totalCount: options?.preserveItems ? current.totalCount : null,
    }));
    try {
      const result = await queryItems(store.get(currentGridQueryAtom), { offset: 0, limit: PAGE_SIZE });
      if (store.get(gridSessionAtom).generation !== generation) return;
      updateSession({
        items: result.items,
        cursor: nextOffset(result.items.length, result.visible_item_count),
        totalCount: result.visible_item_count,
        totalSizeBytes: result.total_size_bytes,
        status: 'idle',
      });
    } catch (error) {
      if (store.get(gridSessionAtom).generation !== generation) return;
      updateSession({ status: 'error', error: error instanceof Error ? error.message : String(error) });
    }
  }

  async loadNextPage(): Promise<void> {
    const before = store.get(gridSessionAtom);
    if (before.cursor == null || before.status === 'appending' || before.status === 'loading') return;
    const offset = before.cursor;
    const generation = before.generation;
    updateSession({ status: 'appending', error: null });
    try {
      const result = await queryItems(store.get(currentGridQueryAtom), { offset, limit: PAGE_SIZE });
      const current = store.get(gridSessionAtom);
      if (current.generation !== generation || current.cursor !== offset) return;
      const items = [...current.items, ...result.items];
      updateSession({
        items,
        cursor: nextOffset(items.length, result.visible_item_count),
        totalCount: result.visible_item_count,
        totalSizeBytes: result.total_size_bytes,
        status: 'idle',
      });
    } catch (error) {
      if (store.get(gridSessionAtom).generation !== generation) return;
      updateSession({ status: 'error', error: error instanceof Error ? error.message : String(error) });
    }
  }

  removeItems(itemIds: number[]): void {
    const remove = new Set(itemIds);
    updateSession((current) => {
      const items = current.items.filter((item) => !remove.has(item.item_id));
      const removed = current.items.length - items.length;
      return removed === 0 ? current : {
        ...current,
        items,
        totalCount: current.totalCount == null ? null : Math.max(0, current.totalCount - removed),
        cursor: nextOffset(items.length, current.totalCount == null ? items.length : current.totalCount - removed),
      };
    });
  }

  async reconcile(_metadataOnly: boolean): Promise<boolean> {
    await this.loadFirstPage({ preserveItems: true });
    return true;
  }

  private setFiltersNow(filters: QueryFilters): void {
    updateSession({ filters: { ...filters, include_tags: [...filters.include_tags], exclude_tags: [...filters.exclude_tags] } });
    void this.loadFirstPage({ preserveItems: true });
  }

  private setSortNow(field: SortField, direction: SortDirection): void {
    updateSession({ sort: { field, direction } });
    this.saveViewPref({ sort_field: field, sort_order: direction });
    void this.loadFirstPage({ preserveItems: true });
  }

  private cancelSearch(): void {
    if (!this.searchTimer) return;
    clearTimeout(this.searchTimer);
    this.searchTimer = null;
  }
}

function nextOffset(loaded: number, visibleCount: number): number | null {
  return loaded < visibleCount ? loaded : null;
}

export const gridController = new GridSessionController();
