/** Single command owner for the canonical grid session. */

import { getDefaultStore } from 'jotai';
import { queryItems } from '../platform/entityApi';
import { getViewPrefs, GRID_DEFAULTS_SCOPE, setViewPrefs } from '../platform/settingsApi';
import type { ViewPrefsDto, ViewPrefsPatch } from '../platform/settingsApi';
import type { EntityViewQuery, ItemSort } from '../shared/types/canonical';
import { clearSelectionAtom } from '../state/selection';
import { libraryInvalidation } from '../runtime/libraryInvalidation';
import { compileGridQuery, itemFiltersEqual } from '../shared/lib/itemFilters';
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
    case 'total_size':
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

function queryForSession(session: GridSessionSnapshot): EntityViewQuery {
  return compileGridQuery(
    session.scope,
    session.filters,
    {
      field: session.sort.field,
      direction: session.sort.direction,
      random_seed: session.sort.randomSeed ?? null,
    },
    session.searchText,
  );
}

class GridSessionController {
  private searchTimer: ReturnType<typeof setTimeout> | null = null;
  private searchInFlight: Promise<void> | null = null;
  private searchQueued = false;
  private preferenceTimer: ReturnType<typeof setTimeout> | null = null;
  private pendingPreferencePatch: ViewPrefsPatch = {};
  private scopeKey = '';
  private navigationRequest = 0;
  private queryVersion = 0;

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
    const queryVersion = ++this.queryVersion;
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
      revision: current.revision,
      status: 'loading',
      error: null,
      generation: current.generation + 1,
      active: true,
    };

    try {
      const result = await this.queryFirstPageUntilCurrent(
        queryForSession(session),
        () => queryVersion === this.queryVersion,
      );
      if (result == null) throw new Error('Grid navigation was superseded');
      return {
        scopeKey,
        session: {
          ...session,
          items: result.items,
          cursor: result.next_cursor,
          totalCount: result.total,
          totalSizeBytes: result.total_size_bytes,
          revision: result.revision,
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
    this.queryVersion += 1;
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
    this.queryVersion += 1;
    store.set(clearSelectionAtom);
    this.cancelSearch();
    this.searchTimer = setTimeout(() => {
      this.searchTimer = null;
      this.runSettledSearch();
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
    const queryVersion = this.queryVersion;
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
      const query = store.get(currentGridQueryAtom);
      const result = await this.queryFirstPageUntilCurrent(
        query,
        () => queryVersion === this.queryVersion && store.get(gridSessionAtom).generation === generation,
      );
      if (result == null) return;
      const current = store.get(gridSessionAtom);
      updateSession({
        items: reuseStablePageItems(current.items, result.items),
        cursor: result.next_cursor,
        totalCount: result.total,
        totalSizeBytes: result.total_size_bytes,
        revision: result.revision,
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
    const cursor = before.cursor;
    const generation = before.generation;
    updateSession({ status: 'appending', error: null });
    try {
      const result = await queryItems(store.get(currentGridQueryAtom), { cursor, limit: PAGE_SIZE });
      const current = store.get(gridSessionAtom);
      if (current.generation !== generation || current.cursor !== cursor) return;
      if (result.revision !== current.revision
        || result.revision < libraryInvalidation.latestRevision('library')) {
        updateSession({ status: 'idle' });
        await this.reconcile();
        return;
      }
      const items = [...current.items, ...result.items];
      const totalCount = current.totalCount ?? result.total;
      updateSession({
        items,
        cursor: result.next_cursor,
        totalCount,
        totalSizeBytes: current.totalSizeBytes ?? result.total_size_bytes,
        revision: result.revision,
        status: 'idle',
      });
    } catch (error) {
      if (store.get(gridSessionAtom).generation !== generation) return;
      updateSession({ status: 'error', error: error instanceof Error ? error.message : String(error) });
    }
  }

  async reconcile(_affectedItemIds: readonly number[] = []): Promise<boolean> {
    const before = store.get(gridSessionAtom);
    if (!before.active || before.status === 'loading' || before.status === 'appending') return false;
    const generation = before.generation;
    const queryVersion = this.queryVersion;

    try {
      const query = store.get(currentGridQueryAtom);
      const result = await this.queryFirstPageUntilCurrent(
        query,
        () => queryVersion === this.queryVersion
          && store.get(gridSessionAtom).generation === generation
          && store.get(gridSessionAtom).active,
      );
      if (result == null) return false;
      const current = store.get(gridSessionAtom);
      if (current.generation !== generation || !current.active) return false;
      const previousById = new Map(current.items.map((item) => [item.root_id, item]));
      const items = result.items.map((item) => {
        const previous = previousById.get(item.root_id);
        return previous != null && itemSummaryEqual(previous, item) ? previous : item;
      });
      const desiredLength = current.cursor == null
        ? result.total
        : Math.min(current.items.length, result.total);
      let nextCursor = result.next_cursor;
      let resultRevision = result.revision;
      while (items.length < desiredLength && nextCursor != null) {
        const remaining = desiredLength - items.length;
        const append = await queryItems(query, {
          cursor: nextCursor,
          limit: Math.min(PAGE_SIZE, desiredLength - items.length),
        });
        if (store.get(gridSessionAtom).generation !== generation) return false;
        if (append.revision !== resultRevision
          || append.revision < libraryInvalidation.latestRevision('library')) {
          return this.reconcile();
        }
        const knownIds = new Set(items.map((item) => item.root_id));
        for (const item of append.items) {
          if (knownIds.has(item.root_id)) continue;
          const previous = previousById.get(item.root_id);
          items.push(previous != null && itemSummaryEqual(previous, item) ? previous : item);
          knownIds.add(item.root_id);
        }
        nextCursor = append.next_cursor;
        resultRevision = Math.max(resultRevision, append.revision);
        if (append.items.length === 0 || remaining === desiredLength - items.length) break;
      }
      items.length = Math.min(items.length, desiredLength);
      updateSession({
        items,
        cursor: items.length < result.total ? nextCursor : null,
        totalCount: result.total,
        totalSizeBytes: result.total_size_bytes,
        revision: resultRevision,
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
    updateSession({ filters: cloneFilters(filters) });
    this.queryVersion += 1;
    store.set(clearSelectionAtom);
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
    this.queryVersion += 1;
    store.set(clearSelectionAtom);
    if (persistSort) this.saveViewPref({ sort_field: field, sort_order: direction });
    void this.loadFirstPage({ preserveItems: true });
  }

  private cancelSearch(): void {
    this.searchQueued = false;
    if (this.searchTimer) {
      clearTimeout(this.searchTimer);
      this.searchTimer = null;
    }
  }

  private runSettledSearch(): void {
    if (this.searchInFlight) {
      this.searchQueued = true;
      return;
    }
    this.searchInFlight = (async () => {
      do {
        this.searchQueued = false;
        await this.loadFirstPage({ preserveItems: true });
      } while (this.searchQueued);
    })().finally(() => {
      this.searchInFlight = null;
    });
  }

  private async queryFirstPageUntilCurrent(
    query: EntityViewQuery,
    isCurrent: () => boolean,
  ) {
    while (isCurrent()) {
      const result = await queryItems(query, { cursor: null, limit: PAGE_SIZE });
      if (!isCurrent()) return null;
      if (result.revision >= libraryInvalidation.latestRevision('library')) return result;
    }
    return null;
  }
}

function itemSummaryEqual(
  left: GridSessionSnapshot['items'][number],
  right: GridSessionSnapshot['items'][number],
): boolean {
  return left.root_id === right.root_id
    && left.kind === right.kind
    && left.lifecycle === right.lifecycle
    && left.name === right.name
    && left.content_hash === right.content_hash
    && left.mime === right.mime
    && left.width === right.width
    && left.height === right.height
    && left.duration_ms === right.duration_ms
    && left.frame_count === right.frame_count
    && JSON.stringify(left.palette) === JSON.stringify(right.palette)
    && left.rating === right.rating
    && left.media_count === right.media_count;
}

function reuseStablePageItems(
  previous: GridSessionSnapshot['items'],
  incoming: GridSessionSnapshot['items'],
): GridSessionSnapshot['items'] {
  const previousById = new Map(previous.map((item) => [item.root_id, item]));
  const stable = incoming.map((item) => {
    const existing = previousById.get(item.root_id);
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

export const gridController = new GridSessionController();
