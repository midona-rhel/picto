/** Single command owner for the canonical grid session. */

import { getDefaultStore } from 'jotai';
import { queryEntityView, reconcileEntityView } from '../platform/entityApi';
import { getViewPrefs, setViewPrefs } from '../platform/settingsApi';
import type { ViewPrefsDto, ViewPrefsPatch } from '../platform/settingsApi';
import type { BaseScope, EntityViewQuery, QueryFilters } from '../shared/types/canonical';
import { clearSelectionAtom } from '../state/selection';
import {
  currentGridQueryAtom,
  gridSessionAtom,
  initialGridView,
  pendingGridIntentAtom,
  type GridIntent,
  type GridSessionSnapshot,
  type GridViewPreferences,
  type SortDirection,
  type SortField,
} from '../state/grid';

const store = getDefaultStore();
const PAGE_SIZE = 500;
const SEARCH_DEBOUNCE_MS = 300;
const VIEW_PREFS_SAVE_DEBOUNCE_MS = 500;

function scopeToKey(scope: BaseScope): string {
  switch (scope.kind) {
    case 'system': return `system:${scope.key === 'all' ? 'active' : scope.key}`;
    case 'folder': return scope.id != null ? `folder:${scope.id}` : '';
    case 'smart_folder': return scope.id != null ? `smart:${scope.id}` : '';
    case 'tag': return scope.key ? `tag:${scope.key}` : '';
    case 'search': return scope.key ? `search:${scope.key}` : '';
  }
}

function updateSession(update: Partial<GridSessionSnapshot> | ((current: GridSessionSnapshot) => GridSessionSnapshot)): void {
  const current = store.get(gridSessionAtom);
  store.set(gridSessionAtom, typeof update === 'function' ? update(current) : { ...current, ...update });
}

function currentQuery(limit = PAGE_SIZE, cursor?: string): EntityViewQuery {
  return { ...store.get(currentGridQueryAtom), page: { limit, cursor } };
}

function viewFromPreferences(scope: BaseScope, prefs: ViewPrefsDto | null, globals: ViewPrefsDto | null): GridViewPreferences {
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
    updateSession({ scope, searchText: '', filters: {}, active: true, generation, status: 'loading', error: null });

    const [prefs, globals] = await Promise.all([
      key ? getViewPrefs(key).catch(() => null) : Promise.resolve(null),
      getViewPrefs('').catch(() => null),
    ]);
    if (store.get(gridSessionAtom).generation !== generation) return;
    const value = (field: keyof ViewPrefsDto) => prefs?.[field] ?? globals?.[field] ?? null;
    updateSession((current) => ({
      ...current,
      sort: {
        field: (value('sort_field') as SortField) || 'date_added',
        direction: (value('sort_order') as SortDirection) || 'desc',
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
      updateSession({ filters: intent.filters });
      void this.loadFirstPage({ preserveItems: true });
      return;
    }
    if (intent.type === 'sort') {
      updateSession({ sort: { field: intent.field, direction: intent.direction } });
      this.saveViewPref({ sort_field: intent.field, sort_order: intent.direction });
      void this.loadFirstPage({ preserveItems: true });
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
      cursor: options?.preserveItems ? current.cursor : null,
      totalCount: options?.preserveItems ? current.totalCount : null,
    }));
    try {
      const result = await queryEntityView(currentQuery());
      if (store.get(gridSessionAtom).generation !== generation) return;
      updateSession({
        items: result.items,
        cursor: result.next_cursor,
        totalCount: result.total_count,
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
    if (!before.cursor || before.status === 'appending' || before.status === 'loading') return;
    const { cursor, generation } = before;
    updateSession({ status: 'appending', error: null });
    try {
      const result = await queryEntityView(currentQuery(PAGE_SIZE, cursor));
      const current = store.get(gridSessionAtom);
      if (current.generation !== generation || current.cursor !== cursor) return;
      updateSession({
        items: [...current.items, ...result.items],
        cursor: result.next_cursor,
        totalCount: result.total_count ?? current.totalCount,
        totalSizeBytes: result.total_size_bytes ?? current.totalSizeBytes,
        status: 'idle',
      });
    } catch (error) {
      if (store.get(gridSessionAtom).generation !== generation) return;
      updateSession({ status: 'error', error: error instanceof Error ? error.message : String(error) });
    }
  }

  removeItems(entityHashes: string[]): void {
    const remove = new Set(entityHashes);
    updateSession((current) => {
      const items = current.items.filter((item) => !remove.has(item.entity_hash));
      const removed = current.items.length - items.length;
      return removed === 0 ? current : {
        ...current,
        items,
        totalCount: current.totalCount == null ? null : Math.max(0, current.totalCount - removed),
      };
    });
  }

  async reconcile(metadataOnly: boolean): Promise<boolean> {
    const before = store.get(gridSessionAtom);
    if (before.items.length === 0) {
      await this.loadFirstPage();
      return true;
    }
    const generation = before.generation + 1;
    updateSession({ generation });
    try {
      const result = await reconcileEntityView(
        currentQuery(before.items.length),
        before.items.map((item) => item.entity_hash),
        metadataOnly,
      );
      if (store.get(gridSessionAtom).generation !== generation) return false;
      if (result.kind === 'no_change') return false;
      if (result.kind === 'patch_rows' && result.items) {
        const updated = new Map(result.items.map((item) => [item.entity_hash, item]));
        updateSession((current) => ({
          ...current,
          items: current.items.map((item) => updated.get(item.entity_hash) ?? item),
        }));
        return false;
      }
      if (result.kind === 'replace_window' && result.page) {
        updateSession({
          items: result.page.items,
          cursor: result.page.next_cursor,
          totalCount: result.page.total_count,
          totalSizeBytes: result.page.total_size_bytes,
        });
        return false;
      }
    } catch {
      // Canonical first-page query below is the only recovery path.
    }
    await this.loadFirstPage({ preserveItems: true });
    return true;
  }

  private cancelSearch(): void {
    if (!this.searchTimer) return;
    clearTimeout(this.searchTimer);
    this.searchTimer = null;
  }
}

export const gridController = new GridSessionController();
