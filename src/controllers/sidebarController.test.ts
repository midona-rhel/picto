import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import type {
  CanonicalNavigationSnapshot,
  CanonicalSidebarCounts,
} from '../shared/types/canonical';
import { setSidebarTreeAtom, sidebarNodesAtom } from '../state/sidebar';

const getNavigation = vi.hoisted(() => vi.fn());
const getSidebarCounts = vi.hoisted(() => vi.fn());
const getNamespaceSummary = vi.hoisted(() => vi.fn());
vi.mock('../platform/navigationApi', () => ({ getNavigation, getSidebarCounts }));
vi.mock('../platform/tagApi', () => ({ getNamespaceSummary }));

import { buildSidebarNodes, sidebarController } from './sidebarController';

const store = getDefaultStore();

const navigation: CanonicalNavigationSnapshot = {
  folders: [{
    folder_id: 4,
    stable_key: 'folder-4',
    name: 'Folder',
    parent_id: null,
    icon: 'folder',
    color: '#fff',
    notes: 'notes',
    cover_root_id: null,
    display_order: 10,
    count: 8,
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
    view: {
      filter: { kind: 'all', value: [] },
      sort: { field: 'name', direction: 'ascending', random_seed: null },
    },
    display_order: 3,
    count: 0,
  }, {
    smart_folder_id: 8,
    name: 'Rated',
    parent_id: 7,
    icon: null,
    color: null,
    notes: null,
    view: {
      filter: { kind: 'clause', value: { clause: 'ratings', ratings: ['one'] } },
      sort: { field: 'name', direction: 'ascending', random_seed: null },
    },
    display_order: 4,
    count: 7,
  }],
  revision: 11,
};

const counts: CanonicalSidebarCounts = {
  all: 9,
  inbox: 2,
  trash: 1,
  recently_viewed: 3,
  untagged: 4,
  uncategorized: 5,
  duplicates: 6,
  folders: [{ folder_id: 4, count: 8 }],
  smart_folders: [{ smart_folder_id: 7, count: 0 }, { smart_folder_id: 8, count: 7 }],
  revision: 11,
};

describe('replacement sidebar reads', () => {
  beforeEach(() => {
    getNavigation.mockReset();
    getSidebarCounts.mockReset();
    getNamespaceSummary.mockReset().mockResolvedValue([
      { namespace_id: 1, name: 'general', tag_count: 5 },
      { namespace_id: 2, name: 'creator', tag_count: 2 },
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

  it('coalesces concurrent sidebar reads', async () => {
    let release!: () => void;
    const pending = new Promise<void>((resolve) => { release = resolve; });
    getNavigation.mockImplementation(async () => { await pending; return navigation; });
    getSidebarCounts.mockResolvedValue(counts);

    const first = sidebarController.fetchTree();
    const second = sidebarController.fetchTree();
    release();
    await Promise.all([first, second]);

    expect(getNavigation).toHaveBeenCalledTimes(1);
    expect(getSidebarCounts).toHaveBeenCalledTimes(1);
    expect(getNamespaceSummary).toHaveBeenCalledTimes(1);
  });
});
