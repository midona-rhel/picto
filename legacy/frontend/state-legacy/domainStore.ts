import { create } from 'zustand';
import type { SidebarNodeDto, SidebarFreshness } from '../shared/types/sidebar';
import type { SmartFolderPredicate } from '../shared/types/api';
import { sidebarController } from '../controllers/sidebarController';
import { tagsController } from '../controllers/tagsController';

interface SmartFolderSummary {
  id: string;
  name: string;
  parent_id: string | null;
  display_order?: number | null;
  icon?: string | null;
  color?: string | null;
  count: number;
  freshness: SidebarFreshness | string;
  predicate?: SmartFolderPredicate;
  localPredicate?: SmartFolderPredicate;
  hasEffectiveRules: boolean;
  hasLocalRules: boolean;
  sort_field?: string | null;
  sort_order?: string | null;
}

interface DomainState {
  // Sidebar counts
  allActiveCount: number;
  inboxCount: number;
  uncategorizedCount: number;
  trashCount: number;
  untaggedCount: number;
  tagsCount: number;
  duplicatesCount: number;

  // Smart folders derived from sidebar tree
  smartFolders: SmartFolderSummary[];
  smartFolderCounts: Record<string, number>;

  // Folder nodes
  folderNodes: SidebarNodeDto[];

  // Raw sidebar tree for custom consumers
  sidebarNodes: SidebarNodeDto[];
  treeEpoch: number;

  // Loading state
  loading: boolean;

  // Actions
  fetchSidebarTree: () => Promise<void>;
  requestRefresh: () => void;
  applySidebarCounts: (counts: { active: number; inbox: number; trash: number }) => void;
  setDuplicatesCount: (count: number) => void;
  /** Eagerly adjust the sidebar tag count (e.g. +1 on create, -1 on delete). */
  adjustTagsCount: (delta: number) => void;
  /** Targeted folder count adjustment — avoids full tree refetch. */
  adjustFolderCount: (folderId: number, delta: number) => void;
  /** Targeted smart folder count update — avoids full tree refetch. */
  setSmartFolderCount: (smartFolderId: number, count: number) => void;
  /** Patch a folder node's properties in-place. */
  patchFolderNode: (folderId: number, patch: { name?: string; icon?: string | null; color?: string | null }) => void;
  /** Remove a folder node from the sidebar tree. */
  removeFolderNode: (folderId: number) => void;
  /** Patch a smart folder's properties in-place. */
  patchSmartFolder: (sfId: number, patch: { name?: string; icon?: string | null; color?: string | null }) => void;
  /** Remove a smart folder from the sidebar tree. */
  removeSmartFolder: (sfId: number) => void;
  /** Insert a new folder node into the sidebar tree. */
  insertFolderNode: (folderId: number, name: string, parentId: number | null, icon?: string | null, color?: string | null) => void;
  /** Insert a new smart folder node into the sidebar tree. */
  insertSmartFolder: (sfId: number, name: string, parentId: number | null, icon?: string | null, color?: string | null) => void;
  /** Move a smart folder to a new parent in the tree. */
  moveSmartFolderNode: (sfId: number, newParentId: number | null) => void;
  /** Reorder smart folder nodes by applying sort order patches. */
  reorderSmartFolderNodes: (moves: [number, number][]) => void;
  /** Reorder folder nodes by applying sort order patches. */
  reorderFolderNodes: (moves: [number, number][]) => void;
  /** Move a folder to a new parent in the tree. */
  moveFolderNode: (folderId: number, newParentId: number | null) => void;
}

const SIDEBAR_REFRESH_DEBOUNCE_MS = 120;
const SIDEBAR_FETCH_STUCK_TIMEOUT_MS = 8000;
const SIDEBAR_OPTIONAL_QUERY_TIMEOUT_MS = 2500;

let sidebarRefreshTimer: ReturnType<typeof setTimeout> | null = null;
let sidebarRefreshQueuedWhileLoading = false;
let sidebarFetchStartedAt = 0;

function withTimeout<T>(promise: Promise<T>, timeoutMs: number, fallback: T): Promise<T> {
  return new Promise<T>((resolve) => {
    let settled = false;
    const timer = setTimeout(() => {
      if (settled) return;
      settled = true;
      resolve(fallback);
    }, timeoutMs);
    void promise
      .then((value) => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        resolve(value);
      })
      .catch(() => {
        if (settled) return;
        settled = true;
        clearTimeout(timer);
        resolve(fallback);
      });
  });
}

export const useDomainStore = create<DomainState>((set, get) => ({
  allActiveCount: 0,
  inboxCount: 0,
  uncategorizedCount: 0,
  trashCount: 0,
  untaggedCount: 0,
  tagsCount: 0,
  duplicatesCount: 0,
  smartFolders: [],
  smartFolderCounts: {},
  folderNodes: [],
  sidebarNodes: [],
  treeEpoch: 0,
  loading: false,

  fetchSidebarTree: async () => {
    if (get().loading) {
      // Recover from hung fetches so sidebar invalidations don't deadlock forever.
      if (Date.now() - sidebarFetchStartedAt > SIDEBAR_FETCH_STUCK_TIMEOUT_MS) {
        set({ loading: false });
      } else {
        sidebarRefreshQueuedWhileLoading = true;
        return;
      }
    }
    if (get().loading) {
      sidebarRefreshQueuedWhileLoading = true;
      return;
    }
    sidebarFetchStartedAt = Date.now();
    set({ loading: true });

    try {
      const tree = await withTimeout(
        sidebarController.getTree(),
        SIDEBAR_FETCH_STUCK_TIMEOUT_MS,
        { nodes: [], tree_epoch: 0, generated_at: new Date(0).toISOString() },
      );
      const [namespaceSummary] = await Promise.all([
        withTimeout(tagsController.getNamespaceSummary(), SIDEBAR_OPTIONAL_QUERY_TIMEOUT_MS, []),
      ]);
      const nodes = tree.nodes;
      const tagsCount = Array.isArray(namespaceSummary)
        ? namespaceSummary.reduce((sum, row) => sum + (typeof row.count === 'number' ? row.count : 0), 0)
        : 0;

      const allNode = nodes.find((n) => n.id === 'system:active' || n.id === 'system:active_files');
      const inboxNode = nodes.find((n) => n.id === 'system:inbox');
      const uncategorizedNode = nodes.find(
        (n) => n.id === 'system:uncategorized' || n.id === 'system:uncategorized_files',
      );
      const trashNode = nodes.find((n) => n.id === 'system:trash');
      const untaggedNode = nodes.find(
        (n) => n.id === 'system:untagged' || n.id === 'system:untagged_files',
      );
      const duplicatesNode = nodes.find((n) => n.id === 'system:duplicates');
      const inboxCount = inboxNode?.count ?? get().inboxCount;
      const uncategorizedCount = uncategorizedNode?.count ?? 0;
      const untaggedCount = untaggedNode?.count ?? 0;

      const smartNodes = nodes.filter((n) => n.kind === 'smart_folder');
      const smartFolders: SmartFolderSummary[] = [];
      const smartFolderCounts: Record<string, number> = {};

      for (const node of smartNodes) {
        const id = node.id.startsWith('smart:') ? node.id.slice('smart:'.length) : node.id;
        const meta = node.meta as Record<string, unknown> | null;
        smartFolders.push({
          id,
          name: node.name,
          parent_id:
            typeof meta?.parent_id === 'number'
              ? String(meta.parent_id)
              : null,
          display_order: node.sort_order ?? null,
          icon: node.icon,
          color: node.color,
          count: node.count ?? 0,
          freshness: node.freshness,
          predicate: meta?.predicate as SmartFolderPredicate | undefined,
          localPredicate: meta?.local_predicate as SmartFolderPredicate | undefined,
          hasEffectiveRules: meta?.has_effective_rules === true,
          hasLocalRules: meta?.has_local_rules === true,
          sort_field: meta?.sort_field as string | null | undefined,
          sort_order: meta?.sort_order as string | null | undefined,
        });
        if (typeof node.count === 'number') {
          smartFolderCounts[id] = node.count;
        }
      }

      const folderNodes = nodes.filter((n) => n.kind === 'folder');

      set({
        allActiveCount: allNode?.count ?? 0,
        inboxCount,
        uncategorizedCount,
        trashCount: trashNode?.count ?? 0,
        untaggedCount,
        tagsCount,
        duplicatesCount: duplicatesNode?.count ?? 0,
        smartFolders,
        smartFolderCounts,
        folderNodes,
        sidebarNodes: nodes,
        treeEpoch: tree.tree_epoch,
        loading: false,
      });

      // Hydrate the Jotai sidebar atoms (new state owner for this slice).
      const { store: jotaiStore } = await import('../state/store');
      const { sidebarNodesAtom, sidebarEpochAtom, tagsCountAtom } = await import('../state/sidebar');
      jotaiStore.set(sidebarNodesAtom, nodes);
      jotaiStore.set(sidebarEpochAtom, tree.tree_epoch);
      jotaiStore.set(tagsCountAtom, tagsCount);
    } catch (e) {
      console.error('Failed to fetch sidebar tree:', e);
      set({ loading: false });
    } finally {
      sidebarFetchStartedAt = 0;
      if (sidebarRefreshQueuedWhileLoading) {
        sidebarRefreshQueuedWhileLoading = false;
        if (sidebarRefreshTimer) clearTimeout(sidebarRefreshTimer);
        sidebarRefreshTimer = setTimeout(() => {
          sidebarRefreshTimer = null;
          void get().fetchSidebarTree();
        }, SIDEBAR_REFRESH_DEBOUNCE_MS);
      }
    }
  },

  requestRefresh: () => {
    // Coalesce repeated sidebar refresh requests from event storms and chained state changes.
    if (sidebarRefreshTimer) clearTimeout(sidebarRefreshTimer);
    sidebarRefreshTimer = setTimeout(() => {
      sidebarRefreshTimer = null;
      void get().fetchSidebarTree();
    }, SIDEBAR_REFRESH_DEBOUNCE_MS);
  },

  applySidebarCounts: (counts) => {
    set({
      allActiveCount: counts.active,
      inboxCount: counts.inbox,
      trashCount: counts.trash,
    });
  },

  setDuplicatesCount: (count) => set({ duplicatesCount: count }),

  adjustTagsCount: (delta: number) => set((s) => ({ tagsCount: Math.max(0, s.tagsCount + delta) })),

  adjustFolderCount: (folderId, delta) => {
    const fid = String(folderId);
    set((s) => ({
      folderNodes: s.folderNodes.map((n) =>
        n.id === `folder:${fid}` ? { ...n, count: (n.count ?? 0) + delta } : n,
      ),
      sidebarNodes: s.sidebarNodes.map((n) =>
        n.id === `folder:${fid}` ? { ...n, count: (n.count ?? 0) + delta } : n,
      ),
    }));
  },

  setSmartFolderCount: (smartFolderId, count) => {
    const sfid = String(smartFolderId);
    set((s) => ({
      smartFolders: s.smartFolders.map((sf) =>
        sf.id === sfid ? { ...sf, count } : sf,
      ),
      smartFolderCounts: { ...s.smartFolderCounts, [sfid]: count },
    }));
  },

  patchFolderNode: (folderId, patch) => {
    const fid = `folder:${folderId}`;
    set((s) => ({
      folderNodes: s.folderNodes.map((n) =>
        n.id === fid ? { ...n, ...patch } : n,
      ),
      sidebarNodes: s.sidebarNodes.map((n) =>
        n.id === fid ? { ...n, ...patch } : n,
      ),
    }));
  },

  removeFolderNode: (folderId) => {
    const fid = `folder:${folderId}`;
    set((s) => ({
      folderNodes: s.folderNodes.filter((n) => n.id !== fid),
      sidebarNodes: s.sidebarNodes.filter((n) => n.id !== fid),
    }));
  },

  patchSmartFolder: (sfId, patch) => {
    const id = String(sfId);
    set((s) => ({
      smartFolders: s.smartFolders.map((sf) =>
        sf.id === id ? { ...sf, ...patch } : sf,
      ),
      sidebarNodes: s.sidebarNodes.map((n) =>
        n.id === `smart:${id}` ? { ...n, ...patch } : n,
      ),
    }));
  },

  removeSmartFolder: (sfId) => {
    const id = String(sfId);
    set((s) => ({
      smartFolders: s.smartFolders.filter((sf) => sf.id !== id),
      smartFolderCounts: Object.fromEntries(
        Object.entries(s.smartFolderCounts).filter(([k]) => k !== id),
      ),
      sidebarNodes: s.sidebarNodes.filter((n) => n.id !== `smart:${id}`),
    }));
  },

  insertFolderNode: (folderId, name, parentId, icon, color) => {
    const node = {
      id: `folder:${folderId}`,
      kind: 'folder' as const,
      parent_id: parentId != null ? `folder:${parentId}` : null,
      name,
      icon: icon ?? null,
      color: color ?? null,
      count: 0,
      freshness: 'exact' as const,
      selectable: true,
    };
    set((s) => ({
      folderNodes: [...s.folderNodes, node],
      sidebarNodes: [...s.sidebarNodes, node],
    }));
  },

  insertSmartFolder: (sfId, name, parentId, icon, color) => {
    const id = String(sfId);
    const node = {
      id: `smart:${id}`,
      kind: 'smart_folder' as const,
      parent_id: parentId != null ? `smart:${parentId}` : null,
      name,
      icon: icon ?? null,
      color: color ?? null,
      count: 0,
      freshness: 'exact' as const,
      selectable: true,
    };
    set((s) => ({
      smartFolders: [...s.smartFolders, {
        id, name, parent_id: parentId != null ? String(parentId) : null,
        count: 0, freshness: 'exact', hasEffectiveRules: false, hasLocalRules: false,
        icon: icon ?? null, color: color ?? null,
      }],
      smartFolderCounts: { ...s.smartFolderCounts, [id]: 0 },
      sidebarNodes: [...s.sidebarNodes, node],
    }));
  },

  moveSmartFolderNode: (sfId, newParentId) => {
    const id = String(sfId);
    const newPid = newParentId != null ? String(newParentId) : null;
    set((s) => ({
      smartFolders: s.smartFolders.map((sf) =>
        sf.id === id ? { ...sf, parent_id: newPid } : sf,
      ),
      sidebarNodes: s.sidebarNodes.map((n) =>
        n.id === `smart:${id}` ? { ...n, parent_id: newPid != null ? `smart:${newPid}` : null } : n,
      ),
    }));
  },

  reorderSmartFolderNodes: (moves) => {
    const orderMap = new Map(moves.map(([id, order]) => [String(id), order]));
    set((s) => ({
      smartFolders: [...s.smartFolders].sort((a, b) =>
        (orderMap.get(a.id) ?? Infinity) - (orderMap.get(b.id) ?? Infinity),
      ),
      sidebarNodes: s.sidebarNodes.map((n) => {
        if (!n.id.startsWith('smart:')) return n;
        const sfId = n.id.slice('smart:'.length);
        const order = orderMap.get(sfId);
        return order != null ? { ...n, sort_order: order } : n;
      }),
    }));
  },

  reorderFolderNodes: (moves) => {
    const orderMap = new Map(moves.map(([id, order]) => [id, order]));
    set((s) => ({
      folderNodes: s.folderNodes.map((n) => {
        const fid = n.id.startsWith('folder:') ? parseInt(n.id.slice('folder:'.length), 10) : 0;
        const order = orderMap.get(fid);
        return order != null ? { ...n, sort_order: order } : n;
      }),
      sidebarNodes: s.sidebarNodes.map((n) => {
        if (!n.id.startsWith('folder:')) return n;
        const fid = parseInt(n.id.slice('folder:'.length), 10);
        const order = orderMap.get(fid);
        return order != null ? { ...n, sort_order: order } : n;
      }),
    }));
  },

  moveFolderNode: (folderId, newParentId) => {
    const fid = `folder:${folderId}`;
    const newPid = newParentId != null ? `folder:${newParentId}` : null;
    set((s) => ({
      folderNodes: s.folderNodes.map((n) =>
        n.id === fid ? { ...n, parent_id: newPid } : n,
      ),
      sidebarNodes: s.sidebarNodes.map((n) =>
        n.id === fid ? { ...n, parent_id: newPid } : n,
      ),
    }));
  },
}));
