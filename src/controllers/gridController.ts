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
  reduceGridSession,
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

function sessionItems(session = store.get(gridSessionAtom)) {
  return session.pages.flatMap((page) => page.items);
}

function sessionCursor(session = store.get(gridSessionAtom)) {
  return session.pages[session.pages.length - 1]?.next_cursor ?? null;
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
    showSubfolders: scope.kind === 'folder'
      ? (value('show_subfolders') as boolean) ?? initialGridView.showSubfolders
      : false,
  };
}

class GridSessionController {
  private searchTimer: ReturnType<typeof setTimeout> | null = null;
  private preferenceTimer: ReturnType<typeof setTimeout> | null = null;
  private pendingPreferencePatch: ViewPrefsPatch = {};
  private scopeKey = '';

  dispatch(intent: GridIntent, commit = false): void | Promise<boolean | void> {
    const needsTransition = intent.type === 'filter'
      || intent.type === 'sort'
      || (intent.type === 'view' && intent.transition === true);
    if (needsTransition && !commit) {
      store.set(pendingGridIntentAtom, intent);
      return;
    }

    switch (intent.type) {
      case 'navigate': return this.navigate(intent.scope);
      case 'search': return this.search(intent.text);
      case 'filter': return this.filter(intent.filters);
      case 'sort': return this.sort(intent.field, intent.direction);
      case 'view': return this.changeView(intent.patch);
      case 'load_next': return this.loadNext();
      case 'reconcile':
        if (intent.impact === 'metadata') return this.reconcileWindow(true);
        if (intent.impact === 'order') return this.reconcileWindow(false);
        return this.loadFirst({ preserveItems: true });
    }
  }

  private async navigate(scope: BaseScope): Promise<void> {
    this.cancelSearch();
    store.set(clearSelectionAtom);
    const key = scopeToKey(scope);
    this.scopeKey = key;
    const generation = store.get(gridSessionAtom).generation + 1;
    updateSession((current) => ({
      ...reduceGridSession(current, { type: 'navigate', scope }),
      active: true,
      generation,
      status: 'loading',
      error: null,
    }));

    const [prefs, globals] = await Promise.all([
      key ? getViewPrefs(key).catch(() => null) : Promise.resolve(null),
      getViewPrefs('').catch(() => null),
    ]);
    if (store.get(gridSessionAtom).generation !== generation) return;
    const value = (field: keyof ViewPrefsDto) => prefs?.[field] ?? globals?.[field] ?? null;
    updateSession((current) => ({
      ...current,
      query: { ...current.query, sort: {
        field: (value('sort_field') as SortField) || 'date_added',
        direction: (value('sort_order') as SortDirection) || 'desc',
      } },
      view: viewFromPreferences(scope, prefs, globals),
    }));
    await this.loadFirst({ generation });
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

  private search(text: string): void {
    updateSession((current) => reduceGridSession(current, { type: 'search', text }));
    this.cancelSearch();
    this.searchTimer = setTimeout(() => {
      this.searchTimer = null;
      void this.loadFirst({ preserveItems: true });
    }, SEARCH_DEBOUNCE_MS);
  }

  private filter(filters: QueryFilters): void {
    updateSession((current) => reduceGridSession(current, { type: 'filter', filters }));
    void this.loadFirst({ preserveItems: true });
  }

  private sort(field: SortField, direction: SortDirection): void {
    updateSession((current) => reduceGridSession(current, { type: 'sort', field, direction }));
    this.queuePreference({ sort_field: field, sort_order: direction });
    void this.loadFirst({ preserveItems: true });
  }

  private changeView(patch: Partial<GridViewPreferences>): void {
    updateSession((current) => reduceGridSession(current, { type: 'view', patch }));
    this.queuePreference({
      ...(patch.mode === undefined ? null : { view_mode: patch.mode }),
      ...(patch.targetSize === undefined ? null : { target_size: patch.targetSize }),
      ...(patch.showName === undefined ? null : { show_name: patch.showName }),
      ...(patch.showResolution === undefined ? null : { show_resolution: patch.showResolution }),
      ...(patch.showExtension === undefined ? null : { show_extension: patch.showExtension }),
      ...(patch.showExtensionLabel === undefined ? null : { show_label: patch.showExtensionLabel }),
      ...(patch.fitThumbnails === undefined ? null : { thumbnail_fit: patch.fitThumbnails ? 'cover' : 'contain' }),
      ...(patch.showSubfolders === undefined ? null : { show_subfolders: patch.showSubfolders }),
    });
  }

  private queuePreference(patch: ViewPrefsPatch): void {
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

  private async loadFirst(options?: { preserveItems?: boolean; generation?: number }): Promise<void> {
    const generation = options?.generation ?? store.get(gridSessionAtom).generation + 1;
    updateSession((current) => ({
      ...current,
      generation,
      status: 'loading',
      error: null,
      pages: options?.preserveItems ? current.pages : [],
    }));
    try {
      const result = await queryEntityView(currentQuery());
      if (store.get(gridSessionAtom).generation !== generation) return;
      updateSession({
        pages: [result],
        status: 'idle',
      });
    } catch (error) {
      if (store.get(gridSessionAtom).generation !== generation) return;
      updateSession({ status: 'error', error: error instanceof Error ? error.message : String(error) });
    }
  }

  private async loadNext(): Promise<void> {
    const before = store.get(gridSessionAtom);
    const cursor = sessionCursor(before);
    if (!cursor || before.status === 'appending' || before.status === 'loading') return;
    const { generation } = before;
    updateSession({ status: 'appending', error: null });
    try {
      const result = await queryEntityView(currentQuery(PAGE_SIZE, cursor));
      const current = store.get(gridSessionAtom);
      if (current.generation !== generation || sessionCursor(current) !== cursor) return;
      updateSession((session) => ({ ...session,
        pages: [...session.pages, result],
        status: 'idle',
      }));
    } catch (error) {
      if (store.get(gridSessionAtom).generation !== generation) return;
      updateSession({ status: 'error', error: error instanceof Error ? error.message : String(error) });
    }
  }

  removeItems(entityHashes: string[]): void {
    const remove = new Set(entityHashes);
    updateSession((current) => {
      const loaded = sessionItems(current);
      const removed = loaded.filter((item) => remove.has(item.entity_hash)).length;
      return removed === 0 ? current : {
        ...current,
        pages: current.pages.map((page, index) => ({
          ...page,
          items: page.items.filter((item) => !remove.has(item.entity_hash)),
          total_count: index === 0 && page.total_count != null
            ? Math.max(0, page.total_count - removed)
            : page.total_count,
        })),
      };
    });
  }

  private async reconcileWindow(metadataOnly: boolean): Promise<boolean> {
    const before = store.get(gridSessionAtom);
    const beforeItems = sessionItems(before);
    if (beforeItems.length === 0) {
      await this.loadFirst();
      return true;
    }
    const generation = before.generation + 1;
    updateSession({ generation });
    try {
      const result = await reconcileEntityView(
        currentQuery(beforeItems.length),
        beforeItems.map((item) => item.entity_hash),
        metadataOnly,
      );
      if (store.get(gridSessionAtom).generation !== generation) return false;
      if (result.kind === 'no_change') return false;
      if (result.kind === 'patch_rows' && result.items) {
        const updated = new Map(result.items.map((item) => [item.entity_hash, item]));
        updateSession((current) => ({
          ...current,
          pages: current.pages.map((page) => ({ ...page,
            items: page.items.map((item) => updated.get(item.entity_hash) ?? item),
          })),
        }));
        return false;
      }
      if (result.kind === 'replace_window' && result.page) {
        updateSession({
          pages: [result.page],
        });
        return false;
      }
    } catch {
      // Canonical first-page query below is the only recovery path.
    }
    await this.loadFirst({ preserveItems: true });
    return true;
  }

  private cancelSearch(): void {
    if (!this.searchTimer) return;
    clearTimeout(this.searchTimer);
    this.searchTimer = null;
  }
}

export const gridController = new GridSessionController();
