/**
 * Navigation history — back/forward stack for scope navigation.
 *
 * Tracks visited sidebar node IDs. Supports browser-like back/forward.
 */

import { atom, getDefaultStore } from 'jotai';
import { activeNodeIdAtom } from './navigation';
import {
  gridFiltersAtom,
  gridFilterLockedAtom,
  gridDrilldownAtom,
  pendingGridIntentAtom,
  pendingGridNavigationAtom,
  type QueryFilters,
} from './grid';
import { nodeIdToGridScope } from '../shared/lib/gridScope';
import { createEmptyItemFilters } from '../shared/lib/itemFilters';
import type { ItemSort } from '../shared/types/generated/application/ItemSort';
import type { GridScrollPosition } from '../shared/types/gridScroll';
import { subscriptionsSelectionAtom, type SubscriptionsSelection } from './subscriptionsWorkspace';
import {
  quickLookSessionAtom,
  viewerDisplayControlsAtom,
  viewerDisplayStateAtom,
  viewerExitTransitionAtom,
  viewerSessionAtom,
} from './viewer';

const SUBSCRIPTIONS_NODE_ID = 'system:subscriptions';

interface HistoryEntry {
  nodeId: string;
  /** Optional grid rendered as a drill-down within the manager identified by nodeId. */
  gridScopeNodeId?: string;
  /** Canonical filters for this grid visit. Omitted for manager workspaces. */
  filters?: QueryFilters;
  /** Explicit presentation sort for synthetic views such as Random. */
  sort?: ItemSort;
  /** Subject selection inside the subscriptions workspace (null = home). */
  subsSelection?: SubscriptionsSelection;
}

function selectionKey(selection: SubscriptionsSelection | undefined): string {
  return selection ? `${selection.kind}:${selection.id}` : 'home';
}

interface HistoryState {
  stack: HistoryEntry[];
  cursor: number;
}

export interface NavigationSessionSnapshot {
  history: HistoryState;
  scrollPositions: Array<[string, GridScrollPosition]>;
}

const historyAtom = atom<HistoryState>({
  stack: [{ nodeId: 'system:active' }],
  cursor: 0,
});

export const canGoBackAtom = atom((get) => get(historyAtom).cursor > 0);
export const canGoForwardAtom = atom((get) => {
  const h = get(historyAtom);
  return h.cursor < h.stack.length - 1;
});

const store = getDefaultStore();

function cloneFilters(filters: QueryFilters): QueryFilters {
  return {
    ...filters,
    include_tags: [...filters.include_tags],
    exclude_tags: [...filters.exclude_tags],
    include_folder_ids: [...filters.include_folder_ids],
    exclude_folder_ids: [...filters.exclude_folder_ids],
    ratings: [...filters.ratings],
    include_mime_types: [...filters.include_mime_types],
    exclude_mime_types: [...filters.exclude_mime_types],
  };
}

function randomSort(): ItemSort {
  return {
    field: 'random',
    direction: 'ascending',
    random_seed: globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random()}`,
  };
}

function filtersKey(filters: QueryFilters | undefined): string {
  return JSON.stringify(filters ?? createEmptyItemFilters());
}

function pushEntry(entry: HistoryEntry): void {
  const h = store.get(historyAtom);
  const stack = h.stack.slice(0, h.cursor + 1);
  const last = stack[stack.length - 1];
  if (
    last?.nodeId === entry.nodeId
    && last.gridScopeNodeId === entry.gridScopeNodeId
    && selectionKey(last.subsSelection) === selectionKey(entry.subsSelection)
    && filtersKey(last.filters) === filtersKey(entry.filters)
  ) return;
  stack.push(entry);
  if (stack.length > 100) stack.shift();
  store.set(historyAtom, { stack, cursor: stack.length - 1 });
}

export function closeTransientViewers() {
  store.set(viewerSessionAtom, null);
  store.set(quickLookSessionAtom, null);
  store.set(viewerDisplayStateAtom, null);
  store.set(viewerDisplayControlsAtom, null);
}

// ── Scroll position per scope ────────────────────────────────────
const scrollPositions = new Map<string, GridScrollPosition>();

export function captureNavigationSession(): NavigationSessionSnapshot {
  const history = store.get(historyAtom);
  return {
    history: {
      cursor: history.cursor,
      stack: history.stack.map((entry) => ({
        ...entry,
        filters: entry.filters ? cloneFilters(entry.filters) : undefined,
        sort: entry.sort ? { ...entry.sort } : undefined,
        subsSelection: entry.subsSelection ? { ...entry.subsSelection } : undefined,
      })),
    },
    scrollPositions: [...scrollPositions.entries()].map(([nodeId, position]) => [nodeId, { ...position }]),
  };
}

export function restoreNavigationSession(snapshot: NavigationSessionSnapshot): void {
  scrollPositions.clear();
  snapshot.scrollPositions.forEach(([nodeId, position]) => scrollPositions.set(nodeId, { ...position }));
  const history: HistoryState = {
    cursor: snapshot.history.cursor,
    stack: snapshot.history.stack.map((entry) => ({
      ...entry,
      filters: entry.filters ? cloneFilters(entry.filters) : undefined,
      sort: entry.sort ? { ...entry.sort } : undefined,
      subsSelection: entry.subsSelection ? { ...entry.subsSelection } : undefined,
    })),
  };
  store.set(historyAtom, history);
  applyHistoryEntry(history.stack[history.cursor]);
}

export function resetNavigationHistory(nodeId = 'system:active'): void {
  scrollPositions.clear();
  store.set(gridDrilldownAtom, null);
  store.set(pendingGridIntentAtom, null);
  store.set(pendingGridNavigationAtom, null);
  store.set(historyAtom, { stack: [{ nodeId }], cursor: 0 });
}

export function saveScrollPosition(nodeId: string, position: GridScrollPosition) {
  scrollPositions.set(nodeId, position);
}

export function getScrollPosition(nodeId: string): GridScrollPosition | null {
  return scrollPositions.get(nodeId) ?? null;
}

/** Remove scopes that no longer exist from history and their saved grid positions. */
export function removeHistoryEntries(nodeIds: Iterable<string>) {
  const removed = new Set(nodeIds);
  if (removed.size === 0) return;

  for (const nodeId of removed) scrollPositions.delete(nodeId);

  const h = store.get(historyAtom);
  const stack = h.stack.filter((entry) => !removed.has(entry.nodeId));
  const retainedThroughCursor = h.stack
    .slice(0, h.cursor + 1)
    .filter((entry) => !removed.has(entry.nodeId)).length;
  const nextStack = stack.length > 0
    ? stack
    : [{ nodeId: 'system:active' }];
  const cursor = Math.min(Math.max(retainedThroughCursor - 1, 0), nextStack.length - 1);
  store.set(historyAtom, { stack: nextStack, cursor });
}

/** Push a new node onto the history stack (called on direct scope navigation, NOT back/forward). */
export function pushHistory(nodeId: string, sort?: ItemSort, filters?: QueryFilters) {
  // Direct navigation to the subscriptions node lands on its home.
  if (nodeId === SUBSCRIPTIONS_NODE_ID) {
    store.set(subscriptionsSelectionAtom, null);
  }
  pushEntry({ nodeId, sort, filters: filters ? cloneFilters(filters) : undefined });
  // Direct navigation = fresh start. Clear saved scroll so grid starts at top.
  // Back/forward never calls pushHistory, so their scroll positions are preserved.
  scrollPositions.delete(nodeId);
}

/** Apply a discrete filter action and make the result traversable with Back/Forward. */
export function navigateWithGridFilters(
  nodeId: string,
  filters: QueryFilters,
  ownerNodeId?: string,
): void {
  const nextFilters = cloneFilters(filters);
  const activeNodeId = store.get(activeNodeIdAtom);
  const h = store.get(historyAtom);
  const current = h.stack[h.cursor];

  // Older entries only stored the scope. Capture the outgoing filter state before appending.
  if (current?.nodeId === activeNodeId && nodeIdToGridScope(activeNodeId)) {
    const stack = [...h.stack];
    stack[h.cursor] = { ...current, filters: cloneFilters(store.get(gridFiltersAtom)) };
    store.set(historyAtom, { ...h, stack });
  }
  pushEntry({
    nodeId: ownerNodeId ?? nodeId,
    gridScopeNodeId: ownerNodeId ? nodeId : undefined,
    filters: nextFilters,
  });
  scrollPositions.delete(nodeId);

  if (ownerNodeId) {
    store.set(pendingGridNavigationAtom, { nodeId, filters: nextFilters });
    store.set(gridDrilldownAtom, { ownerNodeId, scopeNodeId: nodeId, filters: nextFilters });
    if (activeNodeId !== ownerNodeId) store.set(activeNodeIdAtom, ownerNodeId);
    return;
  }

  if (activeNodeId === nodeId) {
    closeTransientViewers();
    store.set(pendingGridIntentAtom, { type: 'filter', filters: nextFilters, restoreScroll: false });
    return;
  }
  if (store.get(viewerSessionAtom) || store.get(quickLookSessionAtom)) {
    store.set(viewerExitTransitionAtom, true);
  }
  store.set(pendingGridNavigationAtom, { nodeId, filters: nextFilters });
  store.set(activeNodeIdAtom, nodeId);
}

/** Navigate to a workspace scope, leaving any item/group viewer first. */
export function navigateToNode(nodeId: string) {
  const hadDrilldown = store.get(gridDrilldownAtom) != null;
  store.set(gridDrilldownAtom, null);
  if (store.get(activeNodeIdAtom) === nodeId) {
    closeTransientViewers();
    store.set(viewerExitTransitionAtom, false);
    if (hadDrilldown) pushHistory(nodeId);
    return;
  }
  if (store.get(viewerSessionAtom) || store.get(quickLookSessionAtom)) {
    store.set(viewerExitTransitionAtom, true);
  }
  const sort = nodeId === 'system:random' ? randomSort() : undefined;
  const lockedFilters = store.get(gridFilterLockedAtom) && nodeIdToGridScope(nodeId)
    ? cloneFilters(store.get(gridFiltersAtom))
    : undefined;
  if (sort || lockedFilters) {
    store.set(pendingGridNavigationAtom, {
      nodeId,
      filters: lockedFilters ?? createEmptyItemFilters(),
      sort,
    });
  }
  store.set(activeNodeIdAtom, nodeId);
  pushHistory(nodeId, sort, lockedFilters);
}

/**
 * Record navigation inside the subscriptions workspace (home and
 * subject pages) so the universal back/forward traverses them too.
 */
export function pushSubscriptionsHistory(selection: SubscriptionsSelection) {
  const h = store.get(historyAtom);
  const stack = h.stack.slice(0, h.cursor + 1);
  const last = stack[stack.length - 1];
  if (last?.nodeId === SUBSCRIPTIONS_NODE_ID && selectionKey(last.subsSelection) === selectionKey(selection ?? undefined)) {
    return;
  }
  stack.push({
    nodeId: SUBSCRIPTIONS_NODE_ID,
    subsSelection: selection ?? undefined,
  });
  if (stack.length > 100) stack.shift();
  store.set(historyAtom, { stack, cursor: stack.length - 1 });
}

function applyHistoryEntry(entry: HistoryEntry) {
  if (store.get(viewerSessionAtom) || store.get(quickLookSessionAtom)) {
    store.set(viewerExitTransitionAtom, true);
  }
  const currentNodeId = store.get(activeNodeIdAtom);
  if (entry.gridScopeNodeId) {
    const filters = cloneFilters(entry.filters ?? createEmptyItemFilters());
    store.set(pendingGridNavigationAtom, {
      nodeId: entry.gridScopeNodeId,
      filters,
      restoreScroll: true,
    });
    store.set(gridDrilldownAtom, {
      ownerNodeId: entry.nodeId,
      scopeNodeId: entry.gridScopeNodeId,
      filters,
    });
    if (currentNodeId !== entry.nodeId) store.set(activeNodeIdAtom, entry.nodeId);
    return;
  }
  store.set(gridDrilldownAtom, null);
  if (nodeIdToGridScope(entry.nodeId)) {
    const filters = cloneFilters(entry.filters ?? createEmptyItemFilters());
    if (currentNodeId === entry.nodeId) {
      store.set(pendingGridIntentAtom, { type: 'filter', filters, restoreScroll: true });
    } else {
      store.set(pendingGridNavigationAtom, {
        nodeId: entry.nodeId,
        filters,
        sort: entry.sort,
        restoreScroll: true,
      });
      store.set(activeNodeIdAtom, entry.nodeId);
    }
  } else {
    store.set(activeNodeIdAtom, entry.nodeId);
  }
  if (entry.nodeId === SUBSCRIPTIONS_NODE_ID) {
    store.set(subscriptionsSelectionAtom, entry.subsSelection ?? null);
  }
}

export function goBack() {
  const h = store.get(historyAtom);
  const next = h.cursor - 1;
  if (next < 0) return;
  store.set(historyAtom, { ...h, cursor: next });
  applyHistoryEntry(h.stack[next]);
}

export function goForward() {
  const h = store.get(historyAtom);
  const next = h.cursor + 1;
  if (next >= h.stack.length) return;
  store.set(historyAtom, { ...h, cursor: next });
  applyHistoryEntry(h.stack[next]);
}
