import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import type { NavigationSnapshot } from '../shared/types/generated/application/NavigationSnapshot';
import type { SidebarCounts } from '../shared/types/generated/application/SidebarCounts';
import { setSidebarTreeAtom, sidebarNodesAtom } from '../state/sidebar';

const getNavigation = vi.hoisted(() => vi.fn());
const getSidebarCounts = vi.hoisted(() => vi.fn());
const getNamespaceSummary = vi.hoisted(() => vi.fn());
vi.mock('../platform/navigationApi', () => ({ getNavigation, getSidebarCounts }));
vi.mock('../platform/tagApi', () => ({ getNamespaceSummary }));

import { buildSidebarNodes, sidebarController } from './sidebarController';

const store = getDefaultStore();

const navigation: NavigationSnapshot = {
  folders: [{
    folder_id: 4,
    name: 'Folder',
    parent_id: null,
    icon: 'folder',
    color: '#fff',
    notes: 'notes',
    sort_rank: 10,
    watch_path: null,
    watch_enabled: false,
    watch_subfolders: false,
  }],
  smart_folders: [{
    smart_folder_id: 7,
    name: 'Smart',
    parent_id: null,
    icon: null,
    color: null,
    notes: null,
    predicate: { groups: [] },
    sort_field: 'name',
    sort_order: 'asc',
    display_order: 3,
  }, {
    smart_folder_id: 8,
    name: 'Rated',
    parent_id: 7,
    icon: null,
    color: null,
    notes: null,
    predicate: { groups: [{ match_mode: 'all', negate: false, rules: [{ field: 'rating', op: 'gte', value: '1', value2: null, values: null }] }] },
    sort_field: 'name',
    sort_order: 'asc',
    display_order: 4,
  }],
  revision: 11,
};

const counts: SidebarCounts = {
  all: 9,
  inbox: 2,
  trash: 1,
  recently_viewed: 3,
  untagged: 4,
  uncategorized: 5,
  duplicates: 6,
  folders: [{ id: 4, count: 8 }],
  smart_folders: [{ id: 7, count: 0 }, { id: 8, count: 7 }],
  revision: 11,
};

describe('replacement sidebar reads', () => {
  beforeEach(() => {
    getNavigation.mockReset();
    getSidebarCounts.mockReset();
    getNamespaceSummary.mockReset().mockResolvedValue([
      { namespace: 'general', count: 5 },
      { namespace: 'creator', count: 2 },
    ]);
    store.set(setSidebarTreeAtom, { nodes: [], epoch: 0 });
  });

  it('maps replacement counts so All remains the active library only', () => {
    const nodes = buildSidebarNodes(navigation, counts, 7);

    expect(nodes.find((node) => node.id === 'system:active')?.count).toBe(9);
    expect(nodes.find((node) => node.id === 'system:inbox')?.count).toBe(2);
    expect(nodes.find((node) => node.id === 'system:tag_manager')?.count).toBe(7);
    expect(nodes.find((node) => node.id === 'system:random')?.count).toBeNull();
    expect(nodes.find((node) => node.id === 'folder:4')?.count).toBe(8);
    expect(nodes.find((node) => node.id === 'smart:7')).toMatchObject({
      count: null,
      selectable: false,
      meta: expect.objectContaining({ is_group: true }),
    });
    expect(nodes.find((node) => node.id === 'smart:8')).toMatchObject({
      count: 7,
      selectable: true,
      meta: expect.objectContaining({ is_group: false }),
    });
  });

  it('reads navigation and counts at the same revision before replacing the tree', async () => {
    getNavigation.mockResolvedValue(navigation);
    getSidebarCounts.mockResolvedValue(counts);

    await sidebarController.fetchTree();

    expect(getNavigation).toHaveBeenCalledWith();
    expect(getSidebarCounts).toHaveBeenCalledWith();
    expect(getNamespaceSummary).toHaveBeenCalledWith();
    expect(store.get(sidebarNodesAtom).map((node) => node.id)).toEqual([
      'system:active',
      'system:inbox',
      'system:recent_viewed',
      'system:uncategorized',
      'system:untagged',
      'system:tag_manager',
      'system:random',
      'system:duplicates',
      'system:trash',
      'folder:4',
      'smart:7',
      'smart:8',
    ]);
  });

  it('keeps current counts when imports advance the revision during the read', async () => {
    getNavigation.mockResolvedValue({ ...navigation, revision: 12 });
    getSidebarCounts.mockResolvedValue(counts);

    await sidebarController.fetchTree();

    expect(store.get(sidebarNodesAtom).find((node) => node.id === 'system:inbox')?.count).toBe(2);
    expect(getNavigation).toHaveBeenCalledTimes(1);
    expect(getSidebarCounts).toHaveBeenCalledTimes(1);
    expect(getNamespaceSummary).toHaveBeenCalledTimes(1);
  });
});
