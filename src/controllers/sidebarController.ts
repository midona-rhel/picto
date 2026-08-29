/** Builds the renderer sidebar from replacement navigation and count reads. */

import { getDefaultStore } from 'jotai';
import { getNavigation, getSidebarCounts } from '../platform/navigationApi';
import { getNamespaceSummary } from '../platform/tagApi';
import type {
  CanonicalNavigationSnapshot,
  CanonicalSidebarCounts,
  FilterExpr,
  SidebarNodeDto,
} from '../shared/types/canonical';
import { setSidebarTreeAtom, sidebarLoadingAtom } from '../state/sidebar';

const store = getDefaultStore();

const SYSTEM_NODES: Array<{ id: string; name: string; sort_order: number }> = [
  { id: 'system:active', name: 'All', sort_order: 0 },
  { id: 'system:inbox', name: 'Inbox', sort_order: 1 },
  { id: 'system:recent_viewed', name: 'Recently Viewed', sort_order: 2 },
  { id: 'system:uncategorized', name: 'Uncategorized', sort_order: 3 },
  { id: 'system:untagged', name: 'Untagged', sort_order: 4 },
  { id: 'system:tag_manager', name: 'Tags', sort_order: 5 },
  { id: 'system:random', name: 'Random', sort_order: 6 },
  { id: 'system:duplicates', name: 'Duplicates', sort_order: 7 },
  { id: 'system:trash', name: 'Trash', sort_order: 8 },
];

function finiteRevision(value: number, label: string): number {
  if (!Number.isSafeInteger(value) || value < 0) throw new Error(`${label} revision is invalid.`);
  return value;
}

function countValue(value: number, label: string): number {
  if (!Number.isSafeInteger(value) || value < 0) throw new Error(`${label} count is invalid.`);
  return value;
}

function systemNodes(counts: CanonicalSidebarCounts, totalTagCount: number): SidebarNodeDto[] {
  const values: Record<string, number | null> = {
    'system:active': countValue(counts.all, 'All'),
    'system:inbox': countValue(counts.inbox, 'Inbox'),
    'system:recent_viewed': countValue(counts.recently_viewed, 'Recently Viewed'),
    'system:uncategorized': countValue(counts.uncategorized, 'Uncategorized'),
    'system:untagged': countValue(counts.untagged, 'Untagged'),
    'system:tag_manager': countValue(totalTagCount, 'Tags'),
    'system:random': null,
    'system:duplicates': countValue(counts.duplicates, 'Duplicates'),
    'system:trash': countValue(counts.trash, 'Trash'),
  };
  return SYSTEM_NODES.map(({ id, name, sort_order }) => ({
    id,
    kind: 'system',
    parent_id: null,
    name,
    sort_order,
    count: values[id],
    freshness: 'exact',
    selectable: true,
  }));
}

function folderNodes(
  navigation: CanonicalNavigationSnapshot,
  counts: CanonicalSidebarCounts,
): SidebarNodeDto[] {
  const countsById = new Map(counts.folders.map((entry) => [entry.folder_id, entry.count]));
  return navigation.folders.map((folder) => ({
    id: `folder:${folder.folder_id}`,
    kind: 'folder',
    parent_id: folder.parent_id == null ? 'section:folders' : `folder:${folder.parent_id}`,
    name: folder.name,
    icon: folder.icon,
    color: folder.color,
    sort_order: folder.display_order,
    count: countValue(countsById.get(folder.folder_id) ?? 0, `Folder ${folder.folder_id}`),
    freshness: 'exact',
    selectable: true,
    meta: {
      notes: folder.notes,
      watch_path: folder.watch_path,
      watch_enabled: folder.watch_enabled,
      watch_subfolders: folder.watch_subfolders,
    },
  }));
}

function isEmptyFilter(filter: FilterExpr): boolean {
  return filter.kind === 'all' && filter.value.length === 0;
}

function smartFolderNodes(
  navigation: CanonicalNavigationSnapshot,
  counts: CanonicalSidebarCounts,
): SidebarNodeDto[] {
  const countsById = new Map(
    counts.smart_folders.map((entry) => [entry.smart_folder_id, entry.count]),
  );
  return navigation.smart_folders.map((folder) => {
    const isGroup = isEmptyFilter(folder.view.filter);
    return {
    id: `smart:${folder.smart_folder_id}`,
    kind: 'smart_folder',
    parent_id: folder.parent_id == null ? 'section:smart_folders' : `smart:${folder.parent_id}`,
    name: folder.name,
    icon: folder.icon,
    color: folder.color,
    sort_order: folder.display_order,
    count: isGroup ? null : countValue(countsById.get(folder.smart_folder_id) ?? 0, `Smart folder ${folder.smart_folder_id}`),
    freshness: 'exact',
    selectable: !isGroup,
    meta: {
      is_group: isGroup,
      parent_id: folder.parent_id,
      notes: folder.notes,
      view: folder.view,
    },
    };
  });
}

export function buildSidebarNodes(
  navigation: CanonicalNavigationSnapshot,
  counts: CanonicalSidebarCounts,
  totalTagCount = 0,
): SidebarNodeDto[] {
  return [
    ...systemNodes(counts, totalTagCount),
    ...folderNodes(navigation, counts),
    ...smartFolderNodes(navigation, counts),
  ];
}

async function readSidebarSnapshot() {
  const [navigation, counts, namespaces] = await Promise.all([
    getNavigation(),
    getSidebarCounts(),
    getNamespaceSummary(),
  ]);
  const navigationRevision = finiteRevision(navigation.revision, 'Navigation');
  const countsRevision = finiteRevision(counts.revision, 'Sidebar');

  // Imports can advance the revision continuously. Navigation and count reads
  // are independently authoritative and the next invalidation reconciles both;
  // discarding them on a revision race leaves the sidebar permanently empty.
  return {
    nodes: buildSidebarNodes(
      navigation,
      counts,
      namespaces.reduce((total, namespace) => total + namespace.tag_count, 0),
    ),
    epoch: Math.max(navigationRevision, countsRevision),
  };
}

let treeFetchPromise: Promise<void> | null = null;
let treeFetchGeneration = 0;

function fetchSidebarTree(replacePending: boolean): Promise<void> {
  if (!replacePending && treeFetchPromise) return treeFetchPromise;

  const generation = ++treeFetchGeneration;
  store.set(sidebarLoadingAtom, true);
  const request = readSidebarSnapshot()
    .then((tree) => {
      if (generation === treeFetchGeneration) store.set(setSidebarTreeAtom, tree);
    })
    .catch((error) => {
      // A replaced read belongs to the previous library mount and must not
      // eject the newly opened library if it happens to fail later.
      if (generation !== treeFetchGeneration) return;
      throw error;
    })
    .finally(() => {
      if (generation !== treeFetchGeneration) return;
      store.set(sidebarLoadingAtom, false);
      treeFetchPromise = null;
    });
  treeFetchPromise = request;
  return request;
}

export const sidebarController = {
  fetchTree() {
    return fetchSidebarTree(false);
  },

  async ensureLoaded() {
    // LibraryGate remounts the application for each opened library. Always
    // replace a read left over from the previous mount, and prevent that stale
    // result from overwriting the newly opened library's navigation.
    try {
      await fetchSidebarTree(true);
    } catch (error) {
      console.warn('[sidebar] initial navigation read failed; retrying once', error);
      await fetchSidebarTree(true);
    }
  },
};
