/**
 * Grid controller — owns grid data loading, sort changes, and reconciliation.
 *
 * All operations that mutate grid state check a monotonic gridVersion token.
 */

import { getDefaultStore } from 'jotai';
import {
  queryEntityView,
  reconcileEntityView,
} from '../platform/entityApi';
import type { BaseScope, EntityViewQuery, QueryFilters } from '../shared/types/canonical';
import {
  getViewPrefs,
  setViewPrefs,
} from '../platform/settingsApi';
import type { ViewPrefsDto, ViewPrefsPatch } from '../platform/settingsApi';
import type { GridViewMode } from '../shared/types/grid';
import type { SortField, SortDirection } from '../state/grid';
import {
  gridScopeAtom, gridActiveAtom, gridItemsAtom, gridCursorAtom,
  gridTotalCountAtom, gridTotalSizeBytesAtom, gridLoadingAtom, gridErrorAtom,
  gridSortFieldAtom, gridSortDirectionAtom, gridSearchTextAtom, gridFiltersAtom,
  gridViewModeAtom, gridTargetSizeAtom,
  gridShowNameAtom, gridShowExtensionAtom, gridShowResolutionAtom,
  gridShowExtensionLabelAtom, gridFitThumbnailsAtom,
  currentGridQueryAtom,
  gridSoftTransitionActionAtom,
} from '../state/grid';
import { clearSelectionAtom } from '../state/selection';

const store = getDefaultStore();

let gridVersion = 0;
let paginationInFlight: string | null = null;
let searchDebounceTimer: ReturnType<typeof setTimeout> | null = null;
const SEARCH_DEBOUNCE_MS = 300;

// Prefetch: keep at least this many items loaded, and start fetching more
// when remaining (unloaded) items drops below the runway threshold.
const PREFETCH_MIN_ITEMS = 5000;
const PREFETCH_RUNWAY_ITEMS = 1000;
const PREFETCH_BATCH_SIZE = 500;
let viewPrefsSaveTimer: ReturnType<typeof setTimeout> | null = null;
const VIEW_PREFS_SAVE_DEBOUNCE_MS = 500;
let currentScopeKey = '';

function scopeToKey(scope: BaseScope): string {
  switch (scope.kind) {
    case 'system': return `system:${scope.key === 'all' ? 'active' : scope.key}`;
    case 'folder': return scope.id != null ? `folder:${scope.id}` : '';
    case 'smart_folder': return scope.id != null ? `smart:${scope.id}` : '';
    default: return '';
  }
}

function currentQuery(limit: number): EntityViewQuery {
  const q = store.get(currentGridQueryAtom);
  return { ...q, page: { limit } };
}

export const gridController = {
  async navigateTo(scope: BaseScope) {
    store.set(gridScopeAtom, scope);
    store.set(gridSearchTextAtom, '');
    store.set(clearSelectionAtom);
    store.set(gridActiveAtom, true);
    if (searchDebounceTimer) { clearTimeout(searchDebounceTimer); searchDebounceTimer = null; }

    // Load persisted view prefs for this scope.
    // Missing fields fall back to global defaults so the previous scope's
    // settings don't bleed into the new one.
    const key = scopeToKey(scope);
    currentScopeKey = key;
    // Load per-scope prefs, then global defaults for any missing fields.
    let prefs: ViewPrefsDto | null = null;
    let globals: ViewPrefsDto | null = null;
    if (key) {
      try { prefs = await getViewPrefs(key); } catch { /* no saved prefs */ }
    }
    try { globals = await getViewPrefs(''); } catch { /* no global prefs */ }
    const p = (field: keyof ViewPrefsDto) => prefs?.[field] ?? globals?.[field] ?? null;
    store.set(gridSortFieldAtom, (p('sort_field') as SortField) || 'date_added');
    store.set(gridSortDirectionAtom, (p('sort_order') as SortDirection) || 'desc');
    store.set(gridViewModeAtom, (p('view_mode') as GridViewMode) || 'waterfall');
    store.set(gridTargetSizeAtom, (p('target_size') as number) ?? 220);
    store.set(gridShowNameAtom, (p('show_name') as boolean) ?? true);
    store.set(gridShowResolutionAtom, (p('show_resolution') as boolean) ?? false);
    store.set(gridShowExtensionAtom, (p('show_extension') as boolean) ?? false);
    store.set(gridShowExtensionLabelAtom, (p('show_label') as boolean) ?? false);
    store.set(gridFitThumbnailsAtom, p('thumbnail_fit') === 'cover');

    await this.loadFirstPage();
  },

  deactivate() {
    gridVersion++;
    paginationInFlight = null;
    if (searchDebounceTimer) { clearTimeout(searchDebounceTimer); searchDebounceTimer = null; }
    store.set(clearSelectionAtom);
    store.set(gridActiveAtom, false);
  },

  /** Update search text and reload with debounce. */
  setSearchText(text: string) {
    store.set(gridSearchTextAtom, text);
    if (searchDebounceTimer) clearTimeout(searchDebounceTimer);
    searchDebounceTimer = setTimeout(() => {
      searchDebounceTimer = null;
      void this.loadFirstPage();
    }, SEARCH_DEBOUNCE_MS);
  },

  /** Replace browse filters at the same soft-fade midpoint used by sorting. */
  setFilters(filters: QueryFilters) {
    store.set(gridSoftTransitionActionAtom, () => {
      store.set(gridFiltersAtom, filters);
      void this.loadFirstPage({ preserveItems: true });
    });
  },

  /** Change sort — deferred to soft fade midpoint. */
  setSort(field: SortField, direction: SortDirection) {
    store.set(gridSoftTransitionActionAtom, () => {
      store.set(gridSortFieldAtom, field);
      store.set(gridSortDirectionAtom, direction);
      void this.loadFirstPage({ preserveItems: true });
    });
    this.saveViewPref({ sort_field: field, sort_order: direction });
  },

  /** Persist a view pref change for the current scope (debounced). */
  saveViewPref(patch: ViewPrefsPatch) {
    if (!currentScopeKey) return;
    const key = currentScopeKey;
    if (viewPrefsSaveTimer) clearTimeout(viewPrefsSaveTimer);
    viewPrefsSaveTimer = setTimeout(() => {
      viewPrefsSaveTimer = null;
      void setViewPrefs(key, patch).catch(() => {});
    }, VIEW_PREFS_SAVE_DEBOUNCE_MS);
  },

  async loadFirstPage(options?: { preserveItems?: boolean }) {
    paginationInFlight = null;
    const v = ++gridVersion;
    store.set(gridLoadingAtom, true);
    store.set(gridErrorAtom, null);
    if (!options?.preserveItems) {
      store.set(gridItemsAtom, []);
      store.set(gridCursorAtom, null);
      store.set(gridTotalCountAtom, null);
      // Don't clear totalSizeBytes — keep previous value visible until new query returns
    }
    try {
      const result = await queryEntityView(currentQuery(PREFETCH_BATCH_SIZE));
      if (v !== gridVersion) return;
      store.set(gridItemsAtom, result.items);
      store.set(gridCursorAtom, result.next_cursor);
      store.set(gridTotalCountAtom, result.total_count);
      store.set(gridTotalSizeBytesAtom, result.total_size_bytes);
      // Kick off background prefetch to fill the buffer
      void this.prefetchToMinimum();
    } catch (err) {
      if (v !== gridVersion) return;
      store.set(gridErrorAtom, err instanceof Error ? err.message : String(err));
    } finally {
      if (v === gridVersion) store.set(gridLoadingAtom, false);
    }
  },

  async loadNextPage() {
    const cursor = store.get(gridCursorAtom);
    if (!cursor) return;
    if (paginationInFlight === cursor) return;
    paginationInFlight = cursor;
    const v = gridVersion;
    store.set(gridLoadingAtom, true);
    try {
      const query = currentQuery(PREFETCH_BATCH_SIZE);
      query.page = { limit: PREFETCH_BATCH_SIZE, cursor };
      const result = await queryEntityView(query);
      if (v !== gridVersion || paginationInFlight !== cursor) return;
      paginationInFlight = null;
      store.set(gridItemsAtom, [...store.get(gridItemsAtom), ...result.items]);
      store.set(gridCursorAtom, result.next_cursor);
      store.set(gridTotalCountAtom, result.total_count);
      store.set(gridTotalSizeBytesAtom, result.total_size_bytes);
      // After each scroll-triggered fetch, check if we need more
      void this.prefetchToMinimum();
    } catch (err) {
      if (v !== gridVersion) return;
      paginationInFlight = null;
      store.set(gridErrorAtom, err instanceof Error ? err.message : String(err));
    } finally {
      if (v === gridVersion) store.set(gridLoadingAtom, false);
    }
  },

  /**
   * Background prefetch loop — keeps loading batches until at least
   * PREFETCH_MIN_ITEMS are buffered or there are no more items.
   * Re-triggered when remaining runway drops below PREFETCH_RUNWAY_ITEMS.
   */
  async prefetchToMinimum() {
    const v = gridVersion;
    while (v === gridVersion) {
      const cursor = store.get(gridCursorAtom);
      if (!cursor) break; // no more pages
      const loaded = store.get(gridItemsAtom).length;
      const total = store.get(gridTotalCountAtom) ?? loaded;
      const remaining = total - loaded;
      // Stop if we have enough loaded or the remaining unloaded items are
      // above the runway (i.e. we're far from the edge)
      if (loaded >= PREFETCH_MIN_ITEMS && remaining > PREFETCH_RUNWAY_ITEMS) break;
      // Also stop if loaded >= total (everything fetched)
      if (loaded >= total) break;
      if (paginationInFlight === cursor) break; // another fetch is handling this cursor
      paginationInFlight = cursor;
      try {
        const query = currentQuery(PREFETCH_BATCH_SIZE);
        query.page = { limit: PREFETCH_BATCH_SIZE, cursor };
        const result = await queryEntityView(query);
        if (v !== gridVersion || paginationInFlight !== cursor) return;
        paginationInFlight = null;
        store.set(gridItemsAtom, [...store.get(gridItemsAtom), ...result.items]);
        store.set(gridCursorAtom, result.next_cursor);
        store.set(gridTotalCountAtom, result.total_count);
        store.set(gridTotalSizeBytesAtom, result.total_size_bytes);
      } catch {
        paginationInFlight = null;
        break; // stop prefetching on error, scroll-triggered fetch will retry
      }
    }
  },

  /** Remove specific items from the grid (trash/delete). */
  removeItems(entityHashes: string[]) {
    const currentItems = store.get(gridItemsAtom);
    const removeSet = new Set(entityHashes);
    const filtered = currentItems.filter((i) => !removeSet.has(i.entity_hash));
    const removedCount = currentItems.length - filtered.length;
    if (removedCount === 0) return;
    store.set(gridItemsAtom, filtered);
    const prevTotal = store.get(gridTotalCountAtom);
    if (prevTotal != null) store.set(gridTotalCountAtom, Math.max(0, prevTotal - removedCount));
  },

  async reconcile(metadataOnly: boolean): Promise<boolean> {
    const items = store.get(gridItemsAtom);
    if (items.length === 0) {
      await this.loadFirstPage({ preserveItems: false });
      return true;
    }

    const v = ++gridVersion;
    const query = currentQuery(items.length);
    const visibleHashes = items.map((i) => i.entity_hash);
    try {
      const result = await reconcileEntityView(query, visibleHashes, metadataOnly);
      if (v !== gridVersion) return false;

      switch (result.kind) {
        case 'no_change':
          return false;
        case 'patch_rows':
          if (result.items) {
            const currentItems = store.get(gridItemsAtom);
            const updated = new Map(result.items.map((i) => [i.entity_hash, i]));
            store.set(gridItemsAtom, currentItems.map((item) => updated.get(item.entity_hash) ?? item));
          }
          return false;
        case 'replace_window':
          if (result.page) {
            store.set(gridItemsAtom, result.page.items);
            store.set(gridCursorAtom, result.page.next_cursor);
            store.set(gridTotalCountAtom, result.page.total_count);
            store.set(gridTotalSizeBytesAtom, result.page.total_size_bytes);
          }
          return false;
        case 'full_refresh_required':
          await this.loadFirstPage({ preserveItems: true });
          return true;
      }
    } catch {
      if (v !== gridVersion) return false;
      await this.loadFirstPage({ preserveItems: true });
      return true;
    }
  },

  async loadSubfolderPreview(folderId: number, limit = 4) {
    return queryEntityView({
      base_scope: { kind: 'folder', id: folderId },
      page: { limit },
    });
  },
};
