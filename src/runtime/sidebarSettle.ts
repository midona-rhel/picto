/**
 * Sidebar runtime settle — listens for backend state_changed events
 * and applies sidebar-relevant updates.
 *
 * This is the only place where backend events touch sidebar state.
 * Controllers handle eager/optimistic updates; this handles confirmed settle.
 */

import { getDefaultStore } from 'jotai';
import { listen } from '../platform/ipc';
import { applySidebarCountsAtom } from '../state/sidebar';
import { sidebarController } from '../controllers/sidebarController';

const store = getDefaultStore();

/** Start listening for sidebar-relevant state changes. Call once at app boot. */
export function startSidebarSettle() {
  listen<{
    changes: { domains?: string[]; compiler_batch_done?: boolean };
    sidebar_counts?: { active: number; inbox: number; trash: number } | null;
  }>('runtime/state_changed', (event) => {
    const { changes, sidebar_counts } = event.payload;

    // Apply direct count updates when the backend sends them
    if (sidebar_counts) {
      store.set(applySidebarCountsAtom, sidebar_counts);
    }

    // Full tree refresh on compiler batch completion (smart folder counts, etc.)
    if (changes.compiler_batch_done) {
      sidebarController.fetchTree();
      return;
    }

    // Targeted tree refresh when sidebar domain is affected
    if (changes.domains?.includes('sidebar')) {
      sidebarController.fetchTree();
    }
  });
}
