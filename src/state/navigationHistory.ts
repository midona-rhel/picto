/**
 * Navigation history — back/forward stack for scope navigation.
 *
 * Tracks visited sidebar node IDs. Supports browser-like back/forward.
 */

import { atom, getDefaultStore } from 'jotai';
import { activeNodeIdAtom } from './navigation';
import { subscriptionsSelectionAtom, type SubscriptionsSelection } from './subscriptionsWorkspace';

const SUBSCRIPTIONS_NODE_ID = 'system:subscriptions';

interface HistoryEntry {
  nodeId: string;
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

// ── Scroll position per scope ────────────────────────────────────
const scrollPositions = new Map<string, number>();

export function saveScrollPosition(nodeId: string, scrollTop: number) {
  scrollPositions.set(nodeId, scrollTop);
}

export function getScrollPosition(nodeId: string): number | null {
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
export function pushHistory(nodeId: string) {
  const h = store.get(historyAtom);
  // If we're not at the end, truncate forward history
  const stack = h.stack.slice(0, h.cursor + 1);
  // Direct navigation to the subscriptions node lands on its home.
  if (nodeId === SUBSCRIPTIONS_NODE_ID) {
    store.set(subscriptionsSelectionAtom, null);
  }
  // Don't push duplicate (for subscriptions, "same" also means same subject)
  const last = stack[stack.length - 1];
  if (last?.nodeId === nodeId && (nodeId !== SUBSCRIPTIONS_NODE_ID || last.subsSelection == null)) {
    return;
  }
  const entry: HistoryEntry = { nodeId };
  stack.push(entry);
  // Cap at 100 entries
  if (stack.length > 100) stack.shift();
  store.set(historyAtom, { stack, cursor: stack.length - 1 });
  // Direct navigation = fresh start. Clear saved scroll so grid starts at top.
  // Back/forward never calls pushHistory, so their scroll positions are preserved.
  scrollPositions.delete(nodeId);
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
  store.set(activeNodeIdAtom, entry.nodeId);
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
