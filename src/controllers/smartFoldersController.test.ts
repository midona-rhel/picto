import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import { activeNodeIdAtom } from '../state/navigation';
import { pendingSidebarRevealNodeIdAtom, sidebarNodesAtom } from '../state/sidebar';

const mocks = vi.hoisted(() => ({
  fetchTree: vi.fn().mockResolvedValue(undefined),
  loadFirstPage: vi.fn().mockResolvedValue(undefined),
  createSmartFolder: vi.fn(),
  updateSmartFolder: vi.fn(),
  announceUndoableMutation: vi.fn().mockResolvedValue(undefined),
}));

vi.mock('./sidebarController', () => ({
  sidebarController: { fetchTree: mocks.fetchTree },
}));
vi.mock('./gridController', () => ({
  gridController: { loadFirstPage: mocks.loadFirstPage },
}));
vi.mock('../platform/smartFolderApi', () => ({
  createSmartFolder: mocks.createSmartFolder,
  deleteSmartFolder: vi.fn(),
  moveSmartFolder: vi.fn(),
  reorderSmartFolders: vi.fn(),
  updateSmartFolder: mocks.updateSmartFolder,
}));
vi.mock('../runtime/historyRuntime', () => ({
  announceUndoableMutation: mocks.announceUndoableMutation,
}));

import { smartFoldersController } from './smartFoldersController';

describe('smart folder refresh', () => {
  beforeEach(() => {
    mocks.fetchTree.mockClear();
    mocks.loadFirstPage.mockClear();
    mocks.createSmartFolder.mockReset();
    mocks.updateSmartFolder.mockReset();
    mocks.announceUndoableMutation.mockClear();
    getDefaultStore().set(activeNodeIdAtom, 'system:active');
    getDefaultStore().set(pendingSidebarRevealNodeIdAtom, null);
  });

  it('keeps the committed smart-folder name visible while the backend settles', async () => {
    let finishUpdate!: () => void;
    mocks.updateSmartFolder.mockImplementation(() => new Promise<void>((resolve) => { finishUpdate = resolve; }));
    getDefaultStore().set(sidebarNodesAtom, [{
      id: 'smart:9', kind: 'smart_folder', parent_id: 'section:smart_folders', name: 'Before',
      sort_order: 0, count: 0, freshness: 'exact', selectable: true,
    }]);
    const payload = {
      name: 'After', parent_id: null, icon: null, color: null, notes: null,
      view: {
        filter: { kind: 'all' as const, value: [] },
        sort: { field: 'name' as const, direction: 'ascending' as const, random_seed: null },
      },
    };

    const update = smartFoldersController.update(9, payload);
    expect(getDefaultStore().get(sidebarNodesAtom)[0]?.name).toBe('After');
    finishUpdate();
    await update;
    expect(getDefaultStore().get(sidebarNodesAtom)[0]?.name).toBe('After');
  });

  it('creates a non-filtering hierarchy group with an empty predicate', async () => {
    mocks.createSmartFolder.mockResolvedValue({ smart_folder_id: 12 });

    await expect(smartFoldersController.createGroup('Reference', 4)).resolves.toBe('smart:12');

    expect(mocks.createSmartFolder).toHaveBeenCalledWith(expect.objectContaining({
      name: 'Reference',
      parent_id: 4,
      view: expect.objectContaining({ filter: { kind: 'all', value: [] } }),
    }));
    expect(mocks.announceUndoableMutation).toHaveBeenCalledWith('smart_folders.create');
    expect(getDefaultStore().get(pendingSidebarRevealNodeIdAtom)).toBe('smart:12');
  });

  it('refreshes counts without navigating to an inactive smart folder', async () => {
    await smartFoldersController.refresh(9);
    expect(mocks.fetchTree).toHaveBeenCalledOnce();
    expect(mocks.loadFirstPage).not.toHaveBeenCalled();
  });

  it('reruns the canonical query for the active smart folder', async () => {
    getDefaultStore().set(activeNodeIdAtom, 'smart:9');
    await smartFoldersController.refresh(9);
    expect(mocks.fetchTree).toHaveBeenCalledOnce();
    expect(mocks.loadFirstPage).toHaveBeenCalledWith({ preserveItems: true });
  });
});
