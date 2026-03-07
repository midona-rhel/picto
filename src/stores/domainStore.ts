import { create } from 'zustand';
import { api } from '#desktop/api';
import type { SidebarNodeDto, SidebarFreshness } from '../types/sidebar';

interface SmartFolderSummary {
  id: string;
  name: string;
  icon?: string | null;
  color?: string | null;
  count: number;
  freshness: SidebarFreshness | string;
  predicate?: unknown;
  sort_field?: string | null;
  sort_order?: string | null;
}

interface DomainState {
  // Sidebar counts
  allImagesCount: number;
  inboxCount: number;
  uncategorizedCount: number;
  trashCount: number;
  untaggedCount: number;
  tagsCount: number;
  recentViewedCount: number;
  duplicatesCount: number;

  // Smart folders derived from sidebar tree
  smartFolders: SmartFolderSummary[];
  smartFolderCounts: Record<string, number>;

  // Folder nodes
  folderNodes: SidebarNodeDto[];

  // Raw sidebar tree for custom consumers
  sidebarNodes: SidebarNodeDto[];
  treeEpoch: number;
  liveInboxImportRuns: number;
  liveInboxFloor: number | null;

  // Loading state
  loading: boolean;

  // Actions
  fetchSidebarTree: () => Promise<void>;
  invalidate: () => void;
  applySidebarCounts: (counts: { all_images: number; inbox: number; trash: number }) => void;
  subscriptionRunStarted: () => void;
  subscriptionRunFinished: () => void;
  setDuplicatesCount: (count: number) => void;
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
  allImagesCount: 0,
  inboxCount: 0,
  uncategorizedCount: 0,
  trashCount: 0,
  untaggedCount: 0,
  tagsCount: 0,
  recentViewedCount: 0,
  duplicatesCount: 0,
  smartFolders: [],
  smartFolderCounts: {},
  folderNodes: [],
  sidebarNodes: [],
  treeEpoch: 0,
  liveInboxImportRuns: 0,
  liveInboxFloor: null,
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
        api.sidebar.getTree(),
        SIDEBAR_FETCH_STUCK_TIMEOUT_MS,
        { nodes: [], tree_epoch: 0, generated_at: new Date(0).toISOString() },
      );
      const [namespaceSummary, inboxCountResp, uncategorizedCountResp, untaggedCountResp, recentViewedCountResp] = await Promise.all([
        withTimeout(api.tags.getNamespaceSummary(), SIDEBAR_OPTIONAL_QUERY_TIMEOUT_MS, []),
        withTimeout(api.grid.getPageSlim({
          limit: 1,
          cursor: null,
          sortField: 'imported_at',
          sortOrder: 'desc',
          status: 'inbox',
        }), SIDEBAR_OPTIONAL_QUERY_TIMEOUT_MS, null),
        withTimeout(api.grid.getPageSlim({
          limit: 1,
          cursor: null,
          sortField: 'imported_at',
          sortOrder: 'desc',
          status: 'uncategorized',
        }), SIDEBAR_OPTIONAL_QUERY_TIMEOUT_MS, null),
        withTimeout(api.grid.getPageSlim({
          limit: 1,
          cursor: null,
          sortField: 'imported_at',
          sortOrder: 'desc',
          status: 'untagged',
        }), SIDEBAR_OPTIONAL_QUERY_TIMEOUT_MS, null),
        withTimeout(api.grid.getPageSlim({
          limit: 1,
          cursor: null,
          sortField: 'imported_at',
          sortOrder: 'desc',
          status: 'recently_viewed',
        }), SIDEBAR_OPTIONAL_QUERY_TIMEOUT_MS, null),
      ]);
      const nodes = tree.nodes;
      const tagsCount = Array.isArray(namespaceSummary)
        ? namespaceSummary.reduce((sum, row) => sum + (typeof row.count === 'number' ? row.count : 0), 0)
        : 0;

      const allNode = nodes.find((n) => n.id === 'system:all' || n.id === 'system:all_files');
      const inboxNode = nodes.find((n) => n.id === 'system:inbox');
      const uncategorizedNode = nodes.find(
        (n) => n.id === 'system:uncategorized' || n.id === 'system:uncategorized_files',
      );
      const trashNode = nodes.find((n) => n.id === 'system:trash');
      const untaggedNode = nodes.find(
        (n) => n.id === 'system:untagged' || n.id === 'system:untagged_files',
      );
      const recentViewedNode = nodes.find(
        (n) => n.id === 'system:recent_viewed' || n.id === 'system:recently_viewed',
      );
      const duplicatesNode = nodes.find((n) => n.id === 'system:duplicates');
      // Prefer the compiled sidebar node count. During subscription imports,
      // the inbox grid snapshot can intentionally stay cached for live insertion,
      // which would otherwise overwrite a fresher sidebar count.
      const resolvedInboxCount = inboxNode?.count ?? inboxCountResp?.total_count ?? get().inboxCount;
      const liveInboxFloor = get().liveInboxFloor;
      const inboxCount = get().liveInboxImportRuns > 0
        ? Math.max(resolvedInboxCount, liveInboxFloor ?? resolvedInboxCount)
        : resolvedInboxCount;
      const uncategorizedCount = uncategorizedCountResp?.total_count ?? uncategorizedNode?.count ?? 0;
      const untaggedCount = untaggedCountResp?.total_count ?? untaggedNode?.count ?? 0;
      const recentViewedCount = recentViewedCountResp?.total_count ?? recentViewedNode?.count ?? 0;

      const smartNodes = nodes.filter((n) => n.kind === 'smart_folder');
      const smartFolders: SmartFolderSummary[] = [];
      const smartFolderCounts: Record<string, number> = {};

      for (const node of smartNodes) {
        const id = node.id.startsWith('smart:') ? node.id.slice('smart:'.length) : node.id;
        const meta = node.meta as Record<string, unknown> | null;
        smartFolders.push({
          id,
          name: node.name,
          icon: node.icon,
          color: node.color,
          count: node.count ?? 0,
          freshness: node.freshness,
          predicate: meta?.predicate,
          sort_field: meta?.sort_field as string | null | undefined,
          sort_order: meta?.sort_order as string | null | undefined,
        });
        if (typeof node.count === 'number') {
          smartFolderCounts[id] = node.count;
        }
      }

      const folderNodes = nodes.filter((n) => n.kind === 'folder');

      set({
        allImagesCount: allNode?.count ?? 0,
        inboxCount,
        uncategorizedCount,
        trashCount: trashNode?.count ?? 0,
        untaggedCount,
        tagsCount,
        recentViewedCount,
        duplicatesCount: duplicatesNode?.count ?? 0,
        smartFolders,
        smartFolderCounts,
        folderNodes,
        sidebarNodes: nodes,
        treeEpoch: tree.tree_epoch,
        loading: false,
      });
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

  invalidate: () => {
    // Coalesce repeated sidebar refresh requests from event storms and chained mutations.
    if (sidebarRefreshTimer) clearTimeout(sidebarRefreshTimer);
    sidebarRefreshTimer = setTimeout(() => {
      sidebarRefreshTimer = null;
      void get().fetchSidebarTree();
    }, SIDEBAR_REFRESH_DEBOUNCE_MS);
  },

  applySidebarCounts: (counts) => {
    const { liveInboxImportRuns, liveInboxFloor } = get();
    set({
      allImagesCount: counts.all_images,
      inboxCount: counts.inbox,
      trashCount: counts.trash,
      liveInboxFloor: liveInboxImportRuns > 0
        ? Math.max(liveInboxFloor ?? counts.inbox, counts.inbox)
        : liveInboxFloor,
    });
  },

  subscriptionRunStarted: () => {
    const { liveInboxImportRuns, liveInboxFloor, inboxCount } = get();
    set({
      liveInboxImportRuns: liveInboxImportRuns + 1,
      liveInboxFloor: liveInboxFloor ?? inboxCount,
    });
  },

  subscriptionRunFinished: () => {
    const nextRuns = Math.max(0, get().liveInboxImportRuns - 1);
    set({
      liveInboxImportRuns: nextRuns,
      liveInboxFloor: nextRuns > 0 ? get().liveInboxFloor : null,
    });
  },

  setDuplicatesCount: (count) => set({ duplicatesCount: count }),
}));
