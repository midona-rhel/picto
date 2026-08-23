/** Builds the renderer sidebar from replacement navigation and count reads. */

import { getDefaultStore } from 'jotai';
import type { NavigationSnapshot } from '../shared/types/generated/application/NavigationSnapshot';
import type { SidebarCounts } from '../shared/types/generated/application/SidebarCounts';
import { getNavigation, getSidebarCounts } from '../platform/navigationApi';
import { reorderSidebarNodes } from '../platform/sidebarApi';
import type { SidebarNodeDto } from '../shared/types/canonical';
import { setSidebarTreeAtom, sidebarLoadingAtom } from '../state/sidebar';

const store = getDefaultStore();

const SYSTEM_NODES: Array<{ id: string; name: string; sort_order: number }> = [
  { id: 'system:active', name: 'All', sort_order: 0 },
  { id: 'system:inbox', name: 'Inbox', sort_order: 1 },
  { id: 'system:recent_viewed', name: 'Recently Viewed', sort_order: 2 },
  { id: 'system:uncategorized', name: 'Uncategorized', sort_order: 3 },
  { id: 'system:untagged', name: 'Untagged', sort_order: 4 },
  { id: 'system:duplicates', name: 'Duplicates', sort_order: 5 },
  { id: 'system:trash', name: 'Trash', sort_order: 6 },
];

function countById(counts: Array<{ id: number; count: number }>): Map<number, number> {
  return new Map(counts.map(({ id, count }) => [id, count]));
}

function finiteRevision(value: number, label: string): number {
  if (!Number.isSafeInteger(value) || value < 0) throw new Error(`${label} revision is invalid.`);
  return value;
}

function countValue(value: number, label: string): number {
  if (!Number.isSafeInteger(value) || value < 0) throw new Error(`${label} count is invalid.`);
  return value;
}

function systemNodes(counts: SidebarCounts): SidebarNodeDto[] {
  const values: Record<string, number> = {
    'system:active': countValue(counts.all, 'All'),
    'system:inbox': countValue(counts.inbox, 'Inbox'),
    'system:recent_viewed': countValue(counts.recently_viewed, 'Recently Viewed'),
    'system:uncategorized': countValue(counts.uncategorized, 'Uncategorized'),
    'system:untagged': countValue(counts.untagged, 'Untagged'),
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

function folderNodes(navigation: NavigationSnapshot, counts: SidebarCounts): SidebarNodeDto[] {
  const countsById = countById(counts.folders);
  return navigation.folders.map((folder) => ({
    id: `folder:${folder.folder_id}`,
    kind: 'folder',
    parent_id: folder.parent_id == null ? 'section:folders' : `folder:${folder.parent_id}`,
    name: folder.name,
    icon: folder.icon,
    color: folder.color,
    sort_order: folder.sort_rank,
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

function smartFolderNodes(navigation: NavigationSnapshot, counts: SidebarCounts): SidebarNodeDto[] {
  const countsById = countById(counts.smart_folders);
  return navigation.smart_folders.map((folder) => ({
    id: `smart:${folder.smart_folder_id}`,
    kind: 'smart_folder',
    parent_id: folder.parent_id == null ? 'section:smart_folders' : `smart:${folder.parent_id}`,
    name: folder.name,
    icon: folder.icon,
    color: folder.color,
    sort_order: folder.display_order,
    count: countValue(countsById.get(folder.smart_folder_id) ?? 0, `Smart folder ${folder.smart_folder_id}`),
    freshness: 'exact',
    selectable: true,
    meta: {
      parent_id: folder.parent_id,
      notes: folder.notes,
      predicate: folder.predicate,
      sort_field: folder.sort_field,
      sort_order: folder.sort_order,
    },
  }));
}

export function buildSidebarNodes(navigation: NavigationSnapshot, counts: SidebarCounts): SidebarNodeDto[] {
  return [
    ...systemNodes(counts),
    ...folderNodes(navigation, counts),
    ...smartFolderNodes(navigation, counts),
  ];
}

async function readSidebarSnapshot() {
  for (let attempt = 0; attempt < 2; attempt += 1) {
    const [navigation, counts] = await Promise.all([getNavigation(), getSidebarCounts()]);
    const navigationRevision = finiteRevision(navigation.revision, 'Navigation');
    const countsRevision = finiteRevision(counts.revision, 'Sidebar');
    if (navigationRevision === countsRevision) {
      return {
        nodes: buildSidebarNodes(navigation, counts),
        epoch: navigationRevision,
      };
    }
  }
  throw new Error('Sidebar navigation changed while it was being read.');
}

let initialFetchDone = false;
let initialFetchPromise: Promise<void> | null = null;

export const sidebarController = {
  async fetchTree() {
    store.set(sidebarLoadingAtom, true);
    try {
      store.set(setSidebarTreeAtom, await readSidebarSnapshot());
    } finally {
      store.set(sidebarLoadingAtom, false);
    }
  },

  ensureLoaded() {
    if (initialFetchDone || initialFetchPromise) return;
    initialFetchPromise = this.fetchTree()
      .then(() => { initialFetchDone = true; })
      .catch(() => {})
      .finally(() => { initialFetchPromise = null; });
  },

  async reorderNodes(moves: [string, number][]) {
    await reorderSidebarNodes(moves);
  },
};
