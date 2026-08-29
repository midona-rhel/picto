import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import { activeNodeIdAtom } from '../state/navigation';

const mocks = vi.hoisted(() => ({
  fetchTree: vi.fn().mockResolvedValue(undefined),
  loadFirstPage: vi.fn().mockResolvedValue(undefined),
  createSmartFolder: vi.fn(),
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
  updateSmartFolder: vi.fn(),
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
    mocks.announceUndoableMutation.mockClear();
    getDefaultStore().set(activeNodeIdAtom, 'system:active');
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
