/**
 * Grid controller — owns grid data loading, sort changes, and reconciliation.
 *
 * All operations that mutate grid state check a monotonic gridVersion token.
 */

import { getDefaultStore } from 'jotai';
import * as api from '../platform/api';
import type { BaseScope, EntityViewQuery } from '../shared/types/canonical';
import type { SortField, SortDirection } from '../state/grid';
import {
  gridScopeAtom, gridActiveAtom, gridItemsAtom, gridCursorAtom,
  gridTotalCountAtom, gridLoadingAtom, gridErrorAtom,
  gridSortFieldAtom, gridSortDirectionAtom,
} from '../state/grid';

const store = getDefaultStore();

let gridVersion = 0;

function currentQuery(limit: number): EntityViewQuery {
  return {
    base_scope: store.get(gridScopeAtom),
    sort: { field: store.get(gridSortFieldAtom), direction: store.get(gridSortDirectionAtom) },
    page: { limit },
  };
}

export const gridController = {
  async navigateTo(scope: BaseScope) {
    store.set(gridScopeAtom, scope);
    store.set(gridActiveAtom, true);
    await this.loadFirstPage();
  },

  deactivate() {
    gridVersion++;
    store.set(gridActiveAtom, false);
  },

  /** Change sort and reload. */
  async setSort(field: SortField, direction: SortDirection) {
    store.set(gridSortFieldAtom, field);
    store.set(gridSortDirectionAtom, direction);
    await this.loadFirstPage();
  },

  async loadFirstPage(options?: { preserveItems?: boolean }) {
    const v = ++gridVersion;
    store.set(gridLoadingAtom, true);
    store.set(gridErrorAtom, null);
    if (!options?.preserveItems) {
      store.set(gridItemsAtom, []);
      store.set(gridCursorAtom, null);
      store.set(gridTotalCountAtom, null);
    }
    try {
      const result = await api.queryEntityView(currentQuery(100));
      if (v !== gridVersion) return;
      store.set(gridItemsAtom, result.items);
      store.set(gridCursorAtom, result.next_cursor);
      store.set(gridTotalCountAtom, result.total_count);
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
    const v = gridVersion;
    store.set(gridLoadingAtom, true);
    try {
      const query = currentQuery(100);
      query.page = { limit: 100, cursor };
      const result = await api.queryEntityView(query);
      if (v !== gridVersion) return;
      store.set(gridItemsAtom, [...store.get(gridItemsAtom), ...result.items]);
      store.set(gridCursorAtom, result.next_cursor);
      store.set(gridTotalCountAtom, result.total_count);
    } catch (err) {
      if (v !== gridVersion) return;
      store.set(gridErrorAtom, err instanceof Error ? err.message : String(err));
    } finally {
      if (v === gridVersion) store.set(gridLoadingAtom, false);
    }
  },

  async reconcile(metadataOnly: boolean): Promise<boolean> {
    const items = store.get(gridItemsAtom);
    if (items.length === 0) return false;

    const v = ++gridVersion;
    const query = currentQuery(items.length);
    const visibleHashes = items.map((i) => i.entity_hash);

    try {
      const result = await api.reconcileEntityView(query, visibleHashes, metadataOnly);
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
};
