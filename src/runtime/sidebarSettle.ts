/**
 * Sidebar runtime settle — applies exact deltas from state_changed events.
 *
 * Count sources:
 *   - sidebar_counts (on engine write events):
 *     active, inbox, trash, uncategorized, untagged — exact
 *     duplicates — exact when duplicate visibility or rows changed; otherwise -1 leaves it intact
 *   - smart_folder_counts (on compiler_batch_done):
 *     per-smart-folder counts after bitmap recompilation — exact
 *
 * Node patch sources:
 *   - sidebar_node_patches: name/icon/color updates, upserts, and removals — exact
 *   - folder_parent_changes / folder_order_changes: reparent and reorder — exact
 *   - smart_folder_parent_changes / smart_folder_order_changes: same — exact
 *
 * Fallback (full tree refresh):
 *   - compiler_batch_done without sidebar_counts or smart_folder_counts (safety net)
 *   - domains:sidebar + domains:folders/smart_folders with folder_ids/smart_folder_ids
 *     but no node patches, parent, or order deltas (should not happen for current
 *     handlers but kept as safety net)
 *   - domains:sidebar alone with no specific delta fields (truly unknown sidebar change)
 */

import { getDefaultStore } from 'jotai';
import { listen } from '../platform/ipc';
import {
  applySidebarCountsAtom,
  applySidebarNodePatchesAtom,
  applySmartFolderCountsAtom,
  applyFolderParentChangesAtom,
  applyFolderOrderChangesAtom,
  applySmartFolderParentChangesAtom,
  applySmartFolderOrderChangesAtom,
} from '../state/sidebar';
import { sidebarController } from '../controllers/sidebarController';
import { settleFolderDeletionFromSidebar } from '../controllers/foldersController';

const store = getDefaultStore();

interface SidebarNodePatch {
  node_id: string;
  removed?: boolean;
  name?: string;
  icon?: string;
  color?: string;
  count?: number | null;
  meta_json?: string | null;
}

interface StateChanges {
  domains?: string[];
  compiler_batch_done?: boolean;
  folder_ids?: number[];
  smart_folder_ids?: number[];
  folder_parent_changes?: Array<[number, number | null]>;
  folder_order_changes?: Array<[number, number]>;
  smart_folder_parent_changes?: Array<[number, number | null]>;
  smart_folder_order_changes?: Array<[number, number]>;
  sidebar_node_patches?: SidebarNodePatch[];
  smart_folder_counts?: Array<[number, number]>;
}

interface SidebarCounts {
  active: number;
  inbox: number;
  trash: number;
  uncategorized: number;
  untagged: number;
  duplicates: number;
}

export function startSidebarSettle(): () => void {
  let cancelled = false;
  const unlistenPromise = listen<{
    origin?: string;
    changes: StateChanges;
    sidebar_counts?: SidebarCounts | null;
  }>('runtime/state_changed', (event) => {
    if (cancelled) return;
    const { changes, sidebar_counts, origin } = event.payload;
    let needsTreeRefresh = false;
    let hadExactDeltas = false;

    // Settle navigation/history before the removal patches erase the tree.
    // Tree expansion also tolerates an incomplete event without preserving stale descendants.
    if (origin === 'delete_folder' && changes.folder_ids?.length) {
      settleFolderDeletionFromSidebar(changes.folder_ids);
      hadExactDeltas = true;
    }

    // 1. System scope counts (from engine write events)
    if (sidebar_counts) {
      store.set(applySidebarCountsAtom, sidebar_counts);
      hadExactDeltas = true;
    }

    // 2. Sidebar node patches (name/icon/color/upsert/remove)
    if (changes.sidebar_node_patches?.length) {
      store.set(applySidebarNodePatchesAtom, changes.sidebar_node_patches);
      hadExactDeltas = true;
    }

    // 3. Folder parent changes
    if (changes.folder_parent_changes?.length) {
      store.set(applyFolderParentChangesAtom, changes.folder_parent_changes);
      hadExactDeltas = true;
    }

    // 4. Folder order changes
    if (changes.folder_order_changes?.length) {
      store.set(applyFolderOrderChangesAtom, changes.folder_order_changes);
      hadExactDeltas = true;
    }

    // 5. Smart folder parent changes
    if (changes.smart_folder_parent_changes?.length) {
      store.set(applySmartFolderParentChangesAtom, changes.smart_folder_parent_changes);
      hadExactDeltas = true;
    }

    // 6. Smart folder order changes
    if (changes.smart_folder_order_changes?.length) {
      store.set(applySmartFolderOrderChangesAtom, changes.smart_folder_order_changes);
      hadExactDeltas = true;
    }

    // 7. Exact smart folder counts emitted after bitmap compilation
    if (changes.smart_folder_counts?.length) {
      store.set(applySmartFolderCountsAtom, changes.smart_folder_counts);
      hadExactDeltas = true;
    }

    // 8. Folder changes with sidebar domain but no exact deltas — SAFETY NET.
    //    All current folder handlers emit either sidebar_node_patches or parent/order
    //    deltas. This branch should not fire for normal operations.
    if (changes.domains?.includes('sidebar') && changes.domains?.includes('folders') && changes.folder_ids?.length) {
      if (!hadExactDeltas) {
        needsTreeRefresh = true;
      }
    }

    // 9. Smart folder changes with sidebar domain but no exact deltas — SAFETY NET.
    if (changes.domains?.includes('sidebar') && changes.domains?.includes('smart_folders') && changes.smart_folder_ids?.length) {
      if (!hadExactDeltas) {
        needsTreeRefresh = true;
      }
    }

    // 10. Compiler batch done without any count deltas — SAFETY NET.
    //     Compiler events should carry sidebar or smart-folder counts. This fires
    //     only if neither is present.
    if (changes.compiler_batch_done) {
      if (!sidebar_counts && !changes.smart_folder_counts?.length) {
        needsTreeRefresh = true;
      }
    }

    // 11. Generic sidebar domain with no specific deltas at all — SAFETY NET.
    //     Fires for truly unknown sidebar changes not covered by any delta path.
    if (changes.domains?.includes('sidebar') && !hadExactDeltas) {
      needsTreeRefresh = true;
    }

    if (needsTreeRefresh) {
      sidebarController.fetchTree();
    }
  });
  return () => {
    cancelled = true;
    unlistenPromise.then((fn) => fn()).catch(() => {});
  };
}
