/** Single command owner for the canonical grid session. */

import { getDefaultStore } from 'jotai';
import { queryItemWindow, supersedeItemWindow } from '../platform/entityApi';
import { getViewPrefs, GRID_DEFAULTS_SCOPE, setViewPrefs } from '../platform/settingsApi';
import type { ViewPrefsDto, ViewPrefsPatch } from '../platform/settingsApi';
import type { EntityViewQuery, ItemSort } from '../shared/types/canonical';
import { clearSelectionAtom } from '../state/selection';
import { libraryInvalidation } from '../runtime/libraryInvalidation';
import {
  compileGridQuery,
  createEmptyItemFilters,
  itemFiltersEqual,
  textSearchQuery,
} from '../shared/lib/itemFilters';
import {
  currentGridQueryAtom,
  gridSessionAtom,
  gridTransitionPhaseAtom,
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
export const GRID_WINDOW_SIZE = 1500;
const SEARCH_DEBOUNCE_MS = 250;
const VIEW_PREFS_SAVE_DEBOUNCE_MS = 500;

export interface PreparedGridNavigation {
  scopeKey: string;
  session: GridSessionSnapshot;
  queryVersion: number;
  preserveSelection?: boolean;
  preferencePatch?: ViewPrefsPatch;
}

interface NavigationOptions {
  filters?: QueryFilters;
  sort?: ItemSort;
  restoreProgress?: number;
}

function windowStartAt(index: number, total: number): number {
  return Math.max(0, Math.min(
    Math.floor((index - PAGE_SIZE) / PAGE_SIZE) * PAGE_SIZE,
    Math.max(0, total - GRID_WINDOW_SIZE),
  ));
}

function scopeToKey(scope: BaseScope): string {
  switch (scope.kind) {
    case 'all': return 'system:active';
    case 'inbox': return 'system:inbox';
    case 'trash': return 'system:trash';
    case 'media_matches': return 'system:active';
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
    // Older builds saved the File Size choice as 'size', which the backend rejects.
    case 'size':
      return 'total_size';
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
  // Inbox starts oldest-first, but a choice made in Inbox belongs to that
  // scope and must remain user-controlled. Do not inherit the application
  // default until the Inbox has an explicit override of its own.
  const configuredField = overrides?.sort_field
    ?? (scope.kind === 'inbox' ? null : defaults?.sort_field)
    ?? null;
  const configuredDirection = overrides?.sort_order
    ?? (scope.kind === 'inbox' ? null : defaults?.sort_order)
    ?? null;
  const field = configuredField == null
    ? fallbackSort.field
    : preferenceSortField(configuredField);
  return {
    sort: {
      field,
      direction: configuredDirection == null
        ? (field === 'folder_order' ? 'ascending' : fallbackSort.direction)
        : preferenceSortDirection(configuredDirection),
    },
    view: viewFromPreferences(scope, defaults, overrides),
  };
}

function cloneFilters(filters: QueryFilters): QueryFilters {
  return {
    ...createEmptyItemFilters(),
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
    session.view.showSubfolders,
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
  private windowRequest = 0;
  private readonly windowChannel = crypto.randomUUID();
  private windowLoad: Promise<void> | null = null;
  private queuedWindow: { start: number; generation: number; request: number } | null = null;
  private activeWindow: { start: number; generation: number; request: number } | null = null;
  private preparationRequest: number | null = null;
  private pendingNavigationVersion: number | null = null;
  private refreshWanted = false;
  private refreshTimer: ReturnType<typeof setTimeout> | null = null;
  private refreshRunning: Promise<boolean> | null = null;
  private lastRefreshTarget = -Infinity;
  private lastRefreshCompletedAt = -Infinity;
  private lastRefreshDurationMs = 0;

  async navigateTo(scope: BaseScope, options?: NavigationOptions): Promise<void> {
    const request = ++this.navigationRequest;
    const prepared = await this.prepareNavigation(scope, options);
    if (request !== this.navigationRequest) return;
    this.commitNavigation(prepared);
  }

  /** Fetch a complete destination without exposing partial state to mounted consumers. */
  async prepareNavigation(
    scope: BaseScope,
    options?: NavigationOptions,
  ): Promise<PreparedGridNavigation> {
    this.cancelWindowRequest();
    this.cancelSearch();
    const queryVersion = ++this.queryVersion;
    const current = store.get(gridSessionAtom);
    const scopeKey = scopeToKey(scope);
    this.pendingNavigationVersion = queryVersion;
    const deadline = Date.now() + 10_000;
    let preferenceTimer: ReturnType<typeof setTimeout> | undefined;
    let preferences: [ViewPrefsDto | null, ViewPrefsDto | null] = [null, null];
    let preferenceError: Error | null = null;
    try {
      preferences = await Promise.race([
        Promise.all([
          getViewPrefs(GRID_DEFAULTS_SCOPE).catch(() => null),
          getViewPrefs(scopeKey).catch(() => null),
        ]),
        new Promise<never>((_, reject) => {
          preferenceTimer = setTimeout(() => reject(new Error('The library view took too long to load. Please retry.')), 10_000);
        }),
      ]);
    } catch (error) {
      preferenceError = error instanceof Error ? error : new Error(String(error));
    } finally {
      if (preferenceTimer != null) clearTimeout(preferenceTimer);
    }
    const [defaults, overrides] = preferences;
    const preferred = stateFromPreferences(scope, defaults, overrides);
    const session: GridSessionSnapshot = {
      ...current,
      scope,
      searchText: '',
      filters: cloneFilters(options?.filters ?? initialGridFilters),
      sort: options?.sort ? {
        field: options.sort.field,
        direction: options.sort.direction,
        randomSeed: options.sort.random_seed,
      } : preferred.sort,
      view: preferred.view,
      items: [],
      queryHeadItems: [],
      windowStart: 0,
      totalCount: null,
      totalSizeBytes: null,
      revision: current.revision,
      status: 'loading',
      error: null,
      generation: current.generation + 1,
      active: true,
    };

    if (preferenceError) return { scopeKey, queryVersion,
      session: { ...session, status: 'error', error: preferenceError.message } };
    return this.prepareSession(scopeKey, session, queryVersion, options?.restoreProgress, deadline);
  }

  /** Sort/view preparation uses the same destination path without mutating the outgoing grid. */
  async prepareIntent(intent: Exclude<GridIntent, { type: 'filter' }>): Promise<PreparedGridNavigation> {
    this.cancelWindowRequest();
    this.cancelSearch();
    const queryVersion = ++this.queryVersion;
    this.pendingNavigationVersion = queryVersion;
    const current = store.get(gridSessionAtom);
    const session: GridSessionSnapshot = {
      ...current,
      generation: current.generation + 1,
      error: null,
      ...(intent.type === 'sort' ? {
        sort: { field: intent.field, direction: intent.direction,
          randomSeed: intent.field === 'random' ? createRandomSeed() : null },
      } : { view: { ...current.view, ...intent.patch } }),
    };
    const changesQuery = intent.type === 'sort'
      || session.view.showSubfolders !== current.view.showSubfolders;
    const prepared = changesQuery
      ? await this.prepareSession(this.scopeKey, session, queryVersion)
      : { scopeKey: this.scopeKey, session, queryVersion };
    return { ...prepared, preserveSelection: intent.type === 'view',
      preferencePatch: intent.type === 'sort'
        ? { sort_field: intent.field, sort_order: intent.direction } : undefined };
  }

  private async prepareSession(
    scopeKey: string,
    session: GridSessionSnapshot,
    queryVersion: number,
    restoreProgress = 0,
    deadline = Date.now() + 10_000,
  ): Promise<PreparedGridNavigation> {
    const progress = Number.isFinite(restoreProgress) ? Math.max(0, Math.min(1, restoreProgress)) : 0;
    const request = this.windowRequest;
    const isCurrent = () => queryVersion === this.queryVersion && request === this.windowRequest;
    if (isCurrent()) this.preparationRequest = request;
    try {
      if (!isCurrent()) throw new Error('Grid navigation was superseded');
      const query = queryForSession(session);
      const result = await queryItemWindow(query, {
        position: progress > 0 ? { kind: 'fraction', value: progress } : { kind: 'ordinal', value: 0 },
        limit: progress > 0 ? GRID_WINDOW_SIZE : PAGE_SIZE,
      }, { channel: this.windowChannel, generation: request }, deadline - Date.now());
      if (!isCurrent()) throw new Error('Grid navigation was superseded');
      return {
        scopeKey,
        queryVersion,
        session: {
          ...session,
          items: result.items,
          queryHeadItems: result.head_items,
          windowStart: result.start,
          totalCount: result.total,
          totalSizeBytes: result.total_size_bytes,
          revision: result.revision,
          status: 'idle',
        },
      };
    } catch (error) {
      return {
        scopeKey,
        queryVersion,
        session: {
          ...session,
          items: [],
          queryHeadItems: [],
          totalCount: null,
          status: 'error',
          error: error instanceof Error ? error.message : String(error),
        },
      };
    } finally {
      if (this.preparationRequest === request) this.preparationRequest = null;
    }
  }

  /** Commit every grid-facing value synchronously while the workspace is hidden. */
  commitNavigation(prepared: PreparedGridNavigation): void {
    if (prepared.queryVersion !== this.queryVersion) return;
    this.pendingNavigationVersion = null;
    this.navigationRequest += 1;
    this.scopeKey = prepared.scopeKey;
    if (!prepared.preserveSelection) store.set(clearSelectionAtom);
    store.set(gridSessionAtom, prepared.session);
    this.observeNewerRevision(prepared.session.revision);
    if (prepared.preferencePatch) this.saveViewPref(prepared.preferencePatch);
  }

  cancelPreparation(): void {
    this.pendingNavigationVersion = null;
    if (this.refreshTimer != null) clearTimeout(this.refreshTimer);
    this.refreshTimer = null;
    this.refreshWanted = false;
    this.lastRefreshTarget = -Infinity;
    this.lastRefreshCompletedAt = -Infinity;
    this.lastRefreshDurationMs = 0;
    this.cancelWindowRequest();
    this.queryVersion += 1;
    this.navigationRequest += 1;
    this.cancelSearch();
  }

  deactivate(): void {
    this.pendingNavigationVersion = null;
    if (this.refreshTimer != null) clearTimeout(this.refreshTimer);
    this.refreshTimer = null;
    this.refreshWanted = false;
    this.lastRefreshTarget = -Infinity;
    this.lastRefreshCompletedAt = -Infinity;
    this.lastRefreshDurationMs = 0;
    this.cancelWindowRequest();
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
    const previousQuery = textSearchQuery(store.get(gridSessionAtom).searchText);
    const nextQuery = textSearchQuery(text);
    updateSession({ searchText: text });
    this.queryVersion += 1;
    store.set(clearSelectionAtom);
    this.cancelSearch();
    if (previousQuery == null && nextQuery == null) return;
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
      void this.prepareIntent(intent).then(prepared => this.commitNavigation(prepared));
      return;
    }
    const changesQuery = intent.patch.showSubfolders != null
      && intent.patch.showSubfolders !== store.get(gridSessionAtom).view.showSubfolders;
    this.updateView(intent.patch);
    if (changesQuery) void this.loadFirstPage();
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
    this.cancelWindowRequest();
    const request = this.windowRequest;
    const queryVersion = this.queryVersion;
    const generation = options?.generation ?? store.get(gridSessionAtom).generation + 1;
    updateSession((current) => ({
      ...current,
      generation,
      status: 'loading',
      error: null,
      items: options?.preserveItems ? current.items : [],
      queryHeadItems: options?.preserveItems ? current.queryHeadItems : [],
      windowStart: options?.preserveItems ? current.windowStart : 0,
      totalCount: options?.preserveItems ? current.totalCount : null,
    }));
    try {
      const query = store.get(currentGridQueryAtom);
      this.preparationRequest = request;
      const result = await queryItemWindow(query, { position: { kind: 'ordinal', value: 0 }, limit: PAGE_SIZE }, {
        channel: this.windowChannel, generation: request,
      });
      if (request === this.preparationRequest) this.preparationRequest = null;
      if (queryVersion !== this.queryVersion || request !== this.windowRequest
        || store.get(gridSessionAtom).generation !== generation) return;
      const current = store.get(gridSessionAtom);
      if (result.revision < current.revision) {
        updateSession({ status: 'error', error: 'The library returned an older view. Please retry.' });
        return;
      }
      updateSession({
        items: reuseStablePageItems(current.items, result.items),
        queryHeadItems: reuseStablePageItems(current.queryHeadItems, result.head_items),
        windowStart: result.start,
        totalCount: result.total,
        totalSizeBytes: result.total_size_bytes,
        revision: result.revision,
        status: 'idle',
      });
      this.observeNewerRevision(result.revision);
    } catch (error) {
      if (store.get(gridSessionAtom).generation !== generation || queryVersion !== this.queryVersion
        || request !== this.windowRequest) return;
      updateSession({ status: 'error', error: error instanceof Error ? error.message : String(error) });
    } finally {
      if (this.preparationRequest === request) this.preparationRequest = null;
    }
  }

  /** Supersede an in-flight destination immediately when scrolling resumes. */
  cancelWindowRequest(): void {
    const needsCancellation = this.activeWindow?.request === this.windowRequest
      || this.preparationRequest === this.windowRequest;
    this.windowRequest++;
    this.queuedWindow = null;
    if (needsCancellation) this.supersedeNativeWindow();
  }

  private supersedeNativeWindow(): void {
    void supersedeItemWindow({ channel: this.windowChannel, generation: this.windowRequest })
      .catch(error => console.warn('Could not supersede grid query', error));
  }

  requestWindowAt(index: number): Promise<void> {
    // Scroll events from the outgoing surface cannot supersede its destination.
    if (this.pendingNavigationVersion != null) return Promise.resolve();
    const current = store.get(gridSessionAtom);
    if (!current.active || current.status === 'loading') return Promise.resolve();
    const start = windowStartAt(index, current.totalCount ?? 0);
    if (start === current.windowStart && current.items.length >= Math.min(GRID_WINDOW_SIZE, current.totalCount ?? 0)) {
      this.cancelWindowRequest();
      return Promise.resolve();
    }
    const pending = this.queuedWindow ?? this.activeWindow;
    if (pending?.start === start && pending.generation === current.generation && pending.request === this.windowRequest) return this.windowLoad ?? Promise.resolve();
    const needsCancellation = this.activeWindow?.request === this.windowRequest;
    this.queuedWindow = { start, generation: current.generation, request: ++this.windowRequest };
    if (needsCancellation) this.supersedeNativeWindow();
    this.windowLoad ??= this.drainWindows().finally(() => { this.windowLoad = null; });
    return this.windowLoad;
  }

  private async drainWindows(): Promise<void> {
    while (this.queuedWindow) {
      const destination = this.queuedWindow;
      this.activeWindow = destination;
      this.queuedWindow = null;
      updateSession({ status: 'appending', error: null });
      try {
        const result = await queryItemWindow(store.get(currentGridQueryAtom), { position: { kind: 'ordinal', value: destination.start }, limit: GRID_WINDOW_SIZE }, {
          channel: this.windowChannel, generation: destination.request,
        });
        const current = store.get(gridSessionAtom);
        if (current.generation !== destination.generation || destination.request !== this.windowRequest || !current.active) continue;
        if (result.revision < current.revision) continue;
        updateSession({
          items: reuseStablePageItems(current.items, result.items), windowStart: result.start,
          queryHeadItems: reuseStablePageItems(current.queryHeadItems, result.head_items),
          totalCount: result.total, totalSizeBytes: result.total_size_bytes,
          revision: result.revision, status: 'idle', error: null,
        });
        this.observeNewerRevision(result.revision);
      } catch (error) {
        if (store.get(gridSessionAtom).generation === destination.generation && destination.request === this.windowRequest) {
          updateSession({ status: 'error', error: error instanceof Error ? error.message : String(error) });
        }
      } finally {
        this.activeWindow = null;
        if (store.get(gridSessionAtom).generation === destination.generation && store.get(gridSessionAtom).status === 'appending') updateSession({ status: 'idle' });
        this.scheduleRefresh();
      }
    }
  }

  reconcile(_affectedItemIds: readonly number[] = []): Promise<boolean> {
    this.refreshWanted = true;
    if (this.refreshRunning) return this.refreshRunning;
    if (this.pendingNavigationVersion != null) return Promise.resolve(false);
    const current = store.get(gridSessionAtom);
    const phase = store.get(gridTransitionPhaseAtom);
    if (phase === 'fading_out' || phase === 'waiting') return Promise.resolve(false);
    if (!current.active || current.status === 'loading' || current.status === 'appending') return Promise.resolve(false);
    this.refreshWanted = false;
    const started = Date.now();
    this.refreshRunning = this.reconcileSnapshot().finally(() => {
      this.refreshRunning = null;
      this.lastRefreshCompletedAt = Date.now();
      this.lastRefreshDurationMs = Math.max(0, this.lastRefreshCompletedAt - started);
      this.scheduleRefresh();
    });
    return this.refreshRunning;
  }

  private async reconcileSnapshot(): Promise<boolean> {
    const before = store.get(gridSessionAtom);
    if (!before.active || before.status === 'loading' || before.status === 'appending') return false;
    const generation = before.generation;
    const queryVersion = this.queryVersion;
    this.cancelWindowRequest();
    const request = this.windowRequest;

    try {
      const query = store.get(currentGridQueryAtom);
      this.preparationRequest = request;
      const result = await queryItemWindow(query, {
        position: { kind: 'ordinal', value: before.windowStart },
        limit: Math.min(GRID_WINDOW_SIZE, Math.max(PAGE_SIZE, before.items.length)),
      }, { channel: this.windowChannel, generation: request });
      if (this.preparationRequest === request) this.preparationRequest = null;
      if (queryVersion !== this.queryVersion || request !== this.windowRequest) return false;
      const current = store.get(gridSessionAtom);
      if (current.generation !== generation || !current.active) return false;
      if (result.revision < current.revision) return false;
      const previousById = new Map(current.items.map((item) => [item.root_id, item]));
      const items = result.items.map((item) => {
        const previous = previousById.get(item.root_id);
        return previous != null && itemSummaryEqual(previous, item) ? previous : item;
      });
      updateSession({
        items,
        queryHeadItems: reuseStablePageItems(current.queryHeadItems, result.head_items),
        windowStart: result.start,
        totalCount: result.total,
        totalSizeBytes: result.total_size_bytes,
        revision: result.revision,
        error: null,
      });
      this.observeNewerRevision(result.revision);
      return true;
    } catch (error) {
      if (store.get(gridSessionAtom).generation === generation && queryVersion === this.queryVersion
        && request === this.windowRequest) {
        updateSession({ error: error instanceof Error ? error.message : String(error) });
      }
      return false;
    } finally {
      if (this.preparationRequest === request) this.preparationRequest = null;
    }
  }

  useManualFolderOrder(): void {
    this.applyIntent({ type: 'sort', field: 'folder_order', direction: 'ascending' });
  }

  private setFiltersNow(filters: QueryFilters): void {
    if (itemFiltersEqual(store.get(gridSessionAtom).filters, filters)) return;
    updateSession({ filters: cloneFilters(filters) });
    this.queryVersion += 1;
    store.set(clearSelectionAtom);
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

  private observeNewerRevision(revision: number): void {
    const latest = libraryInvalidation.latestRevision('library');
    if (latest > revision && latest > this.lastRefreshTarget) {
      this.lastRefreshTarget = latest;
      this.refreshWanted = true;
    }
    this.scheduleRefresh();
  }

  /** Committed invalidations coalesce here; the event adapter owns no scheduler. */
  requestReconcile(): void {
    this.refreshWanted = true;
    this.scheduleRefresh();
  }

  flushPendingRefresh(): void {
    this.scheduleRefresh();
  }

  private scheduleRefresh(): void {
    const current = store.get(gridSessionAtom);
    const phase = store.get(gridTransitionPhaseAtom);
    if (!this.refreshWanted || this.refreshTimer != null || this.refreshRunning
      || this.pendingNavigationVersion != null
      || phase === 'fading_out' || phase === 'waiting'
      || !current.active || current.status === 'loading' || current.status === 'appending') return;
    // Fast edits should not leave a stale grid for two seconds. Under sustained
    // writes, spend at most roughly a quarter of time refreshing (up to a one
    // second cooldown); keep one coalesced successor, not a request per event.
    this.refreshTimer = setTimeout(() => {
      this.refreshTimer = null;
      if (this.refreshWanted) void this.reconcile();
    }, Math.max(40, Math.min(1_000, this.lastRefreshDurationMs * 3)
      - (Date.now() - this.lastRefreshCompletedAt)));
  }
}

function itemSummaryEqual(
  left: GridSessionSnapshot['items'][number],
  right: GridSessionSnapshot['items'][number],
): boolean {
  return left.root_id === right.root_id
    && left.kind === right.kind
    && left.lifecycle === right.lifecycle
    && left.cover_media_id === right.cover_media_id
    && left.name === right.name
    && left.content_hash === right.content_hash
    && left.mime === right.mime
    && left.width === right.width
    && left.height === right.height
    && left.duration_ms === right.duration_ms
    && left.frame_count === right.frame_count
    && JSON.stringify(left.palette) === JSON.stringify(right.palette)
    && left.rating === right.rating
    && left.media_count === right.media_count
    && left.total_size_bytes === right.total_size_bytes
    && left.imported_at_ms === right.imported_at_ms
    && left.captured_at_ms === right.captured_at_ms
    && left.modified_at_ms === right.modified_at_ms;
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
