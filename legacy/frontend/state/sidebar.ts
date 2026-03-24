/**
 * Sidebar state — authoritative tree with derived views.
 */

import { atom } from 'jotai';
import type { SidebarNodeDto, SidebarFreshness } from '../shared/types/sidebar';
import type { SmartFolderPredicate } from '../features/smart-folders/components/types';

export interface SmartFolderSummary {
  id: string;
  name: string;
  parent_id: string | null;
  display_order: number | null;
  icon: string | null;
  color: string | null;
  count: number;
  freshness: SidebarFreshness | string;
  predicate?: SmartFolderPredicate;
  localPredicate?: SmartFolderPredicate;
  hasEffectiveRules: boolean;
  hasLocalRules: boolean;
  sort_field?: string | null;
  sort_order?: string | null;
}

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

/** Smart folder summaries derived from the sidebar tree. Extracts
 *  predicate, sort, and rule metadata from the node's meta JSON. */
export const smartFoldersAtom = atom<SmartFolderSummary[]>((get) => {
  const nodes = get(sidebarNodesAtom).filter((n) => n.kind === 'smart_folder');
  return nodes.map((node) => {
    const id = node.id.startsWith('smart:') ? node.id.slice('smart:'.length) : node.id;
    const meta = node.meta as Record<string, unknown> | null;
    return {
      id,
      name: node.name,
      parent_id: typeof meta?.parent_id === 'number' ? String(meta.parent_id) : null,
      display_order: node.sort_order ?? null,
      icon: node.icon ?? null,
      color: node.color ?? null,
      count: node.count ?? 0,
      freshness: node.freshness,
      predicate: meta?.predicate as SmartFolderPredicate | undefined,
      localPredicate: meta?.local_predicate as SmartFolderPredicate | undefined,
      hasEffectiveRules: meta?.has_effective_rules === true,
      hasLocalRules: meta?.has_local_rules === true,
      sort_field: meta?.sort_field as string | null | undefined,
      sort_order: meta?.sort_order as string | null | undefined,
    };
  });
});

/** Smart folder counts keyed by id string. */
export const smartFolderCountsAtom = atom<Record<string, number>>((get) => {
  const folders = get(smartFoldersAtom);
  const counts: Record<string, number> = {};
  for (const sf of folders) {
    counts[sf.id] = sf.count;
  }
  return counts;
});

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

/** Set the duplicates node count. */
export const setDuplicatesCountAtom = atom(
  null,
  (get, set, count: number) => {
    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).map((n) =>
        n.id === 'system:duplicates' ? { ...n, count } : n,
      ),
    );
  },
);

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

/** Insert a new smart folder node. */
export const insertSmartFolderAtom = atom(
  null,
  (get, set, { sfId, name, parentId, icon, color }: {
    sfId: number; name: string; parentId: number | null; icon: string | null; color: string | null;
  }) => {
    const node: SidebarNodeDto = {
      id: `smart:${sfId}`,
      kind: 'smart_folder',
      name,
      parent_id: parentId != null ? String(parentId) : null,
      icon,
      color,
      count: 0,
      sort_order: null,
      freshness: 'fresh',
      selectable: true,
      expanded_by_default: false,
      meta: null,
    };
    set(sidebarNodesAtom, [...get(sidebarNodesAtom), node]);
  },
);

/** Remove a smart folder node. */
export const removeSmartFolderAtom = atom(
  null,
  (get, set, sfId: number) => {
    const sid = `smart:${sfId}`;
    set(sidebarNodesAtom, get(sidebarNodesAtom).filter((n) => n.id !== sid));
  },
);

/** Patch a smart folder node's properties. */
export const patchSmartFolderAtom = atom(
  null,
  (get, set, { sfId, patch }: { sfId: number; patch: { name?: string; icon?: string | null; color?: string | null } }) => {
    const sid = `smart:${sfId}`;
    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).map((n) => (n.id === sid ? { ...n, ...patch } : n)),
    );
  },
);

/** Move a smart folder to a new parent. */
export const moveSmartFolderNodeAtom = atom(
  null,
  (get, set, { sfId, newParentId }: { sfId: number; newParentId: number | null }) => {
    const sid = `smart:${sfId}`;
    const newPid = newParentId != null ? String(newParentId) : null;
    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).map((n) => (n.id === sid ? { ...n, parent_id: newPid } : n)),
    );
  },
);

/** Reorder smart folder nodes. */
export const reorderSmartFolderNodesAtom = atom(
  null,
  (get, set, moves: [number, number][]) => {
    const orderMap = new Map(moves.map(([id, order]) => [`smart:${id}`, order]));
    set(
      sidebarNodesAtom,
      get(sidebarNodesAtom).map((n) => {
        const newOrder = orderMap.get(n.id);
        return newOrder != null ? { ...n, sort_order: newOrder } : n;
      }),
    );
  },
);
