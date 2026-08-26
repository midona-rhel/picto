/** Single command owner for the canonical grid session. */

import { getDefaultStore } from 'jotai';
import { queryItems } from '../platform/entityApi';
import { getViewPrefs, GRID_DEFAULTS_SCOPE, setViewPrefs } from '../platform/settingsApi';
import type { ViewPrefsDto, ViewPrefsPatch } from '../platform/settingsApi';
import type { ItemSort } from '../shared/types/generated/application/ItemSort';
import type { ItemQuery } from '../shared/types/generated/application/ItemQuery';
import { clearSelectionAtom } from '../state/selection';
import { itemFiltersEqual } from '../shared/lib/itemFilters';
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
const SEARCH_DEBOUNCE_MS = 100;
const VIEW_PREFS_SAVE_DEBOUNCE_MS = 500;

export interface PreparedGridNavigation {
  scopeKey: string;
  session: GridSessionSnapshot;
}

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
  defaults: ViewPrefsDto | null,
  overrides: ViewPrefsDto | null,
): GridViewPreferences {
  const value = (field: keyof ViewPrefsDto) => overrides?.[field] ?? defaults?.[field] ?? null;
  return {
    mode: (value('view_mode') as GridViewPreferences['mode']) || initialGridView.mode,
    targetSize: (value('target_size') as number) ?? initialGridView.targetSize,
    spacing: value('spacing') === 'tight' || value('spacing') === 'wide'
      ? value('spacing') as GridViewPreferences['spacing']
      : null,
    showName: (value('show_name') as boolean) ?? initialGridView.showName,
    showResolution: (value('show_resolution') as boolean) ?? initialGridView.showResolution,
    showExtension: (value('show_extension') as boolean) ?? initialGridView.showExtension,
    showExtensionLabel: (value('show_label') as boolean) ?? initialGridView.showExtensionLabel,
    showItemCount: (value('show_item_count') as boolean) ?? initialGridView.showItemCount,
    fitThumbnails: value('thumbnail_fit') === 'cover',
    showSubfolders: scope.kind === 'folder'
      ? (value('show_subfolders') as boolean) ?? initialGridView.showSubfolders
      : false,
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

function defaultSort(scope: BaseScope): { field: SortField; direction: SortDirection } {
  if (scope.kind === 'inbox') return { field: 'imported_at', direction: 'ascending' };
  return { field: 'imported_at', direction: 'descending' };
}

function stateFromPreferences(
  scope: BaseScope,
  defaults: ViewPrefsDto | null,
  overrides: ViewPrefsDto | null,
) {
  const fallbackSort = defaultSort(scope);
  const configuredField = overrides?.sort_field ?? defaults?.sort_field ?? null;
  const configuredDirection = overrides?.sort_order ?? defaults?.sort_order ?? null;
  const field = scope.kind === 'inbox' || configuredField == null
    ? fallbackSort.field
    : preferenceSortField(configuredField);
  return {
    sort: {
      field,
      direction: scope.kind === 'inbox'
        ? fallbackSort.direction
        : configuredDirection == null
        ? (field === 'folder_order' ? 'ascending' : fallbackSort.direction)
        : preferenceSortDirection(configuredDirection),
    },
    view: viewFromPreferences(scope, defaults, overrides),
  };
}

function cloneFilters(filters: QueryFilters): QueryFilters {
  return {
    ...filters,
    include_tags: [...filters.include_tags],
    exclude_tags: [...filters.exclude_tags],
  };
}

function queryForSession(session: GridSessionSnapshot): ItemQuery {
  const searchText = session.searchText.trim();
  return {
    scope: session.scope,
    filters: {
      ...session.filters,
      text: searchText || session.filters.text || null,
    },
    sort: {
      field: session.sort.field,
      direction: session.sort.direction,
      random_seed: session.sort.randomSeed ?? null,
    },
  };
}

class GridSessionController {
  private searchTimer: ReturnType<typeof setTimeout> | null = null;
  private preferenceTimer: ReturnType<typeof setTimeout> | null = null;
  private pendingPreferencePatch: ViewPrefsPatch = {};
  private scopeKey = '';
  private navigationRequest = 0;

  async navigateTo(scope: BaseScope, options?: { filters?: QueryFilters; sort?: ItemSort }): Promise<void> {
    const request = ++this.navigationRequest;
    const prepared = await this.prepareNavigation(scope, options);
    if (request !== this.navigationRequest) return;
    this.commitNavigation(prepared);
  }

  /** Fetch a complete destination without exposing partial state to mounted consumers. */
  async prepareNavigation(
    scope: BaseScope,
    options?: { filters?: QueryFilters; sort?: ItemSort },
  ): Promise<PreparedGridNavigation> {
    this.cancelSearch();
    const current = store.get(gridSessionAtom);
    const scopeKey = scopeToKey(scope);
    const [defaults, overrides] = await Promise.all([
      getViewPrefs(GRID_DEFAULTS_SCOPE).catch(() => null),
      getViewPrefs(scopeKey).catch(() => null),
    ]);
    const preferred = stateFromPreferences(scope, defaults, overrides);
    const session: GridSessionSnapshot = {
      ...current,
      scope,
      searchText: '',
      filters: cloneFilters(options?.filters ?? initialGridFilters),
      sort: options?.sort && scope.kind !== 'inbox' ? {
        field: options.sort.field,
        direction: options.sort.direction,
        randomSeed: options.sort.random_seed,
      } : preferred.sort,
      view: preferred.view,
      items: [],
      cursor: null,
      totalCount: null,
      totalSizeBytes: null,
      status: 'loading',
      error: null,
      generation: current.generation + 1,
      active: true,
    };

    try {
      const result = await queryItems(queryForSession(session), { offset: 0, limit: PAGE_SIZE });
      if (result.visible_item_count == null || result.total_size_bytes == null) {
        throw new Error('The first grid page did not include exact totals');
      }
      return {
        scopeKey,
        session: {
          ...session,
          items: result.items,
          cursor: nextOffset(result.items.length, result.visible_item_count),
          totalCount: result.visible_item_count,
          totalSizeBytes: result.total_size_bytes,
          status: 'idle',
        },
      };
    } catch (error) {
      return {
        scopeKey,
        session: {
          ...session,
          status: 'error',
          error: error instanceof Error ? error.message : String(error),
        },
      };
    }
  }

  /** Commit every grid-facing value synchronously while the workspace is hidden. */
  commitNavigation(prepared: PreparedGridNavigation): void {
    this.navigationRequest += 1;
    this.scopeKey = prepared.scopeKey;
    store.set(clearSelectionAtom);
    store.set(gridSessionAtom, prepared.session);
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
      if (result.visible_item_count == null || result.total_size_bytes == null) {
        throw new Error('The first grid page did not include exact totals');
      }
      const current = store.get(gridSessionAtom);
      updateSession({
        items: reuseStablePageItems(current.items, result.items),
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
      const totalCount = current.totalCount ?? result.visible_item_count ?? items.length;
      updateSession({
        items,
        cursor: nextOffset(items.length, totalCount),
        totalCount,
        totalSizeBytes: current.totalSizeBytes ?? result.total_size_bytes,
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

  async reconcile(affectedItemIds: readonly number[] = []): Promise<boolean> {
    const before = store.get(gridSessionAtom);
    if (!before.active || before.status === 'loading' || before.status === 'appending') return false;
    const generation = before.generation;

    try {
      const query = store.get(currentGridQueryAtom);
      const result = await queryItems(query, { offset: 0, limit: PAGE_SIZE });
      const current = store.get(gridSessionAtom);
      if (current.generation !== generation || !current.active) return false;
      if (result.visible_item_count == null || result.total_size_bytes == null) {
        throw new Error('The reconciled grid page did not include exact totals');
      }
      const previousById = new Map(current.items.map((item) => [item.item_id, item]));
      const refreshed = result.items.map((item) => {
        const previous = previousById.get(item.item_id);
        return previous != null && itemSummaryEqual(previous, item) ? previous : item;
      });
      const refreshedIds = new Set(refreshed.map((item) => item.item_id));
      const affectedIds = new Set(affectedItemIds);
      const items = [
        ...refreshed,
        ...current.items.filter((item) => !refreshedIds.has(item.item_id) && !affectedIds.has(item.item_id)),
      ];
      const desiredLength = current.cursor == null
        ? result.visible_item_count
        : Math.min(current.items.length, result.visible_item_count);
      if (items.length < desiredLength) {
        const append = await queryItems(query, {
          offset: items.length,
          limit: Math.min(PAGE_SIZE, desiredLength - items.length),
        });
        if (store.get(gridSessionAtom).generation !== generation) return false;
        const knownIds = new Set(items.map((item) => item.item_id));
        for (const item of append.items) {
          if (knownIds.has(item.item_id)) continue;
          const previous = previousById.get(item.item_id);
          items.push(previous != null && itemSummaryEqual(previous, item) ? previous : item);
          knownIds.add(item.item_id);
        }
      }
      items.length = Math.min(items.length, desiredLength);
      updateSession({
        items,
        cursor: nextOffset(items.length, result.visible_item_count),
        totalCount: result.visible_item_count,
        totalSizeBytes: result.total_size_bytes,
        error: null,
      });
      return true;
    } catch (error) {
      if (store.get(gridSessionAtom).generation === generation) {
        updateSession({ error: error instanceof Error ? error.message : String(error) });
      }
      return false;
    }
  }

  useManualFolderOrder(): void {
    this.setSortNow('folder_order', 'ascending');
  }

  private setFiltersNow(filters: QueryFilters): void {
    if (itemFiltersEqual(store.get(gridSessionAtom).filters, filters)) return;
    updateSession({ filters: { ...filters, include_tags: [...filters.include_tags], exclude_tags: [...filters.exclude_tags] } });
    void this.loadFirstPage({ preserveItems: true });
  }

  private setSortNow(field: SortField, direction: SortDirection): void {
    const scope = store.get(gridSessionAtom).scope;
    const persistSort = scope.kind !== 'inbox';
    if (scope.kind === 'inbox') {
      field = 'imported_at';
      direction = 'ascending';
    }
    updateSession({
      sort: {
        field,
        direction,
        randomSeed: field === 'random' ? createRandomSeed() : null,
      },
    });
    if (persistSort) this.saveViewPref({ sort_field: field, sort_order: direction });
    void this.loadFirstPage({ preserveItems: true });
  }

  private cancelSearch(): void {
    if (!this.searchTimer) return;
    clearTimeout(this.searchTimer);
    this.searchTimer = null;
  }
}

function itemSummaryEqual(
  left: GridSessionSnapshot['items'][number],
  right: GridSessionSnapshot['items'][number],
): boolean {
  return left.item_id === right.item_id
    && left.kind === right.kind
    && left.lifecycle === right.lifecycle
    && left.name === right.name
    && left.display_file_hash === right.display_file_hash
    && left.display_mime_type === right.display_mime_type
    && left.pixel_width === right.pixel_width
    && left.pixel_height === right.pixel_height
    && left.duration_ms === right.duration_ms
    && left.frame_count === right.frame_count
    && left.dominant_color_hex === right.dominant_color_hex
    && left.rating === right.rating
    && left.media_count === right.media_count;
}

function reuseStablePageItems(
  previous: GridSessionSnapshot['items'],
  incoming: GridSessionSnapshot['items'],
): GridSessionSnapshot['items'] {
  const previousById = new Map(previous.map((item) => [item.item_id, item]));
  const stable = incoming.map((item) => {
    const existing = previousById.get(item.item_id);
    return existing != null && itemSummaryEqual(existing, item) ? existing : item;
  });
  return stable.length === previous.length
    && stable.every((item, index) => item === previous[index])
    ? previous
    : stable;
}

function createRandomSeed(): string {
  return globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random()}`;
}

function nextOffset(loaded: number, visibleCount: number): number | null {
  return loaded < visibleCount ? loaded : null;
}

export const gridController = new GridSessionController();
