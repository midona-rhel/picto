/**
 * Sidebar controller — owns backend-facing sidebar actions.
 *
 * Calls the API layer, then mutates state atoms.
 * Does not own visible state — that is src/state/sidebar.ts.
 */

import { getDefaultStore } from 'jotai';
import { getSidebarTree, reorderSidebarNodes } from '../platform/sidebarApi';
import { setSidebarTreeAtom, sidebarLoadingAtom } from '../state/sidebar';

const store = getDefaultStore();

let initialFetchDone = false;
let initialFetchPromise: Promise<void> | null = null;

export const sidebarController = {
  /** Fetch the sidebar tree. First call is idempotent (module-level guard). */
  async fetchTree() {
    store.set(sidebarLoadingAtom, true);
    try {
      const response = await getSidebarTree();
      store.set(setSidebarTreeAtom, {
        nodes: response.nodes,
        epoch: response.tree_epoch,
      });
    } finally {
      store.set(sidebarLoadingAtom, false);
    }
  },

  /** Load the sidebar tree exactly once. Safe to call from StrictMode effects.
   *  Resets the guard on failure so a retry is possible. */
  ensureLoaded() {
    if (initialFetchDone) return;
    if (initialFetchPromise) return;
    initialFetchPromise = this.fetchTree()
      .then(() => { initialFetchDone = true; })
      .catch(() => { /* allow retry on next call */ })
      .finally(() => { initialFetchPromise = null; });
  },

  /** Reorder sidebar nodes. */
  async reorderNodes(moves: [string, number][]) {
    await reorderSidebarNodes(moves);
  },
};
