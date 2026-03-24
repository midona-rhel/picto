/**
 * Sidebar state — authoritative tree with derived views.
 */

import { atom } from 'jotai';
import type { SidebarNodeDto } from '../shared/types/sidebar';

// ── Authoritative state ────────────────────────────────────────

/** The full sidebar tree from the backend. */
export const sidebarNodesAtom = atom<SidebarNodeDto[]>([]);

/** Tree epoch — incremented on each backend refresh. */
export const sidebarEpochAtom = atom(0);

/** Whether a sidebar fetch is in flight. */
export const sidebarLoadingAtom = atom(false);

// ── Derived state ──────────────────────────────────────────────

/** Folder nodes filtered from the sidebar tree. */
export const folderNodesAtom = atom((get) =>
  get(sidebarNodesAtom).filter((n) => n.kind === 'folder'),
);

/** Smart folder nodes filtered from the sidebar tree. */
export const smartFolderNodesAtom = atom((get) =>
  get(sidebarNodesAtom).filter((n) => n.kind === 'smart_folder'),
);

/** System scope counts derived from sidebar nodes. */
export const scopeCountsAtom = atom((get) => {
  const nodes = get(sidebarNodesAtom);
  const find = (id: string) => nodes.find((n) => n.id === id)?.count ?? 0;
  return {
    active: find('system:active') || find('system:active_files'),
    inbox: find('system:inbox'),
    trash: find('system:trash'),
    uncategorized: find('system:uncategorized') || find('system:uncategorized_files'),
    untagged: find('system:untagged') || find('system:untagged_files'),
    duplicates: find('system:duplicates'),
  };
});

/** Tag count (total across all namespaces). Stored separately — comes
 *  from a different backend query than the sidebar tree. */
export const tagsCountAtom = atom(0);

// ── Actions ────────────────────────────────────────────────────

/** Eagerly adjust a folder's count without a full tree refetch. */
export const adjustFolderCountAtom = atom(
  null,
  (get, set, { folderId, delta }: { folderId: number; delta: number }) => {
    const fid = `folder:${folderId}`;
    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).map((n) =>
        n.id === fid ? { ...n, count: (n.count ?? 0) + delta } : n,
      ),
    );
  },
);

/** Patch a folder node's properties in-place. */
export const patchFolderNodeAtom = atom(
  null,
  (get, set, { folderId, patch }: { folderId: number; patch: Partial<SidebarNodeDto> }) => {
    const fid = `folder:${folderId}`;
    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).map((n) => (n.id === fid ? { ...n, ...patch } : n)),
    );
  },
);

/** Remove a folder node from the tree. */
export const removeFolderNodeAtom = atom(
  null,
  (get, set, folderId: number) => {
    const fid = `folder:${folderId}`;
    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).filter((n) => n.id !== fid),
    );
  },
);

/** Insert a new folder node into the tree. */
export const insertFolderNodeAtom = atom(
  null,
  (get, set, node: SidebarNodeDto) => {
    set(sidebarNodesAtom, [...get(sidebarNodesAtom), node]);
  },
);

/** Apply sidebar counts from a backend event or eager adjustment. */
export const applySidebarCountsAtom = atom(
  null,
  (get, set, counts: { active: number; inbox: number; trash: number }) => {
    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).map((n) => {
        if (n.id === 'system:active' || n.id === 'system:active_files') return { ...n, count: counts.active };
        if (n.id === 'system:inbox') return { ...n, count: counts.inbox };
        if (n.id === 'system:trash') return { ...n, count: counts.trash };
        return n;
      }),
    );
  },
);

/** Reorder folder nodes by applying sort_order patches. */
export const reorderFolderNodesAtom = atom(
  null,
  (get, set, moves: [number, number][]) => {
    const orderMap = new Map(moves.map(([id, order]) => [`folder:${id}`, order]));
    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).map((n) => {
        const newOrder = orderMap.get(n.id);
        return newOrder != null ? { ...n, sort_order: newOrder } : n;
      }),
    );
  },
);

/** Move a folder node to a new parent. */
export const moveFolderNodeAtom = atom(
  null,
  (get, set, { folderId, newParentId }: { folderId: number; newParentId: number | null }) => {
    const fid = `folder:${folderId}`;
    const newPid = newParentId != null ? `folder:${newParentId}` : null;
    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).map((n) =>
        n.id === fid ? { ...n, parent_id: newPid } : n,
      ),
    );
  },
);

/** Eagerly set a smart folder's count. */
export const setSmartFolderCountAtom = atom(
  null,
  (get, set, { smartFolderId, count }: { smartFolderId: number; count: number }) => {
    const sid = `smart:${smartFolderId}`;
    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).map((n) => (n.id === sid ? { ...n, count } : n)),
    );
  },
);
