/**
 * Navigation history — back/forward stack for scope navigation.
 *
 * Tracks visited sidebar node IDs. Supports browser-like back/forward.
 * Validates collection scopes before navigating (skips deleted collections).
 */

import { atom, getDefaultStore } from 'jotai';
import { collectionsController } from '../controllers/collectionsController';
import { activeNodeIdAtom, parentNodeIdAtom, collectionNameAtom } from './navigation';

interface HistoryEntry {
  nodeId: string;
  parentNodeId: string | null;
  collectionName: string | null;
}

interface HistoryState {
  stack: HistoryEntry[];
  cursor: number;
}

const historyAtom = atom<HistoryState>({
  stack: [{ nodeId: 'system:active', parentNodeId: null, collectionName: null }],
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

/** Check if a nodeId points to a scope that still exists. */
async function isValidNodeId(nodeId: string): Promise<boolean> {
  if (!nodeId.startsWith('collection:')) return true;
  const id = parseInt(nodeId.slice(11), 10);
  if (isNaN(id)) return false;
  return collectionsController.exists(id);
}

/** Push a new node onto the history stack (called on direct scope navigation, NOT back/forward). */
export function pushHistory(nodeId: string) {
  const h = store.get(historyAtom);
  // If we're not at the end, truncate forward history
  const stack = h.stack.slice(0, h.cursor + 1);
  // Don't push duplicate
  if (stack[stack.length - 1]?.nodeId === nodeId) return;
  // Capture current collection context
  const entry: HistoryEntry = {
    nodeId,
    parentNodeId: store.get(parentNodeIdAtom),
    collectionName: store.get(collectionNameAtom),
  };
  stack.push(entry);
  // Cap at 100 entries
  if (stack.length > 100) stack.shift();
  store.set(historyAtom, { stack, cursor: stack.length - 1 });
  // Direct navigation = fresh start. Clear saved scroll so grid starts at top.
  // Back/forward never calls pushHistory, so their scroll positions are preserved.
  scrollPositions.delete(nodeId);
}

function applyHistoryEntry(entry: HistoryEntry) {
  store.set(parentNodeIdAtom, entry.parentNodeId);
  store.set(collectionNameAtom, entry.collectionName);
  store.set(activeNodeIdAtom, entry.nodeId);
}

/** Go back in history, skipping invalid entries (e.g. deleted collections). */
export async function goBack() {
  const h = store.get(historyAtom);
  let next = h.cursor - 1;
  while (next >= 0) {
    if (await isValidNodeId(h.stack[next].nodeId)) break;
    next--;
  }
  if (next < 0) return;
  store.set(historyAtom, { ...h, cursor: next });
  applyHistoryEntry(h.stack[next]);
}

/** Go forward in history, skipping invalid entries (e.g. deleted collections). */
export async function goForward() {
  const h = store.get(historyAtom);
  let next = h.cursor + 1;
  while (next < h.stack.length) {
    if (await isValidNodeId(h.stack[next].nodeId)) break;
    next++;
  }
  if (next >= h.stack.length) return;
  store.set(historyAtom, { ...h, cursor: next });
  applyHistoryEntry(h.stack[next]);
}
