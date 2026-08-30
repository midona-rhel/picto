import { getDefaultStore } from 'jotai';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { activeNodeIdAtom } from '../state/navigation';
import { pendingSidebarRevealNodeIdAtom, sidebarNodesAtom } from '../state/sidebar';
import { clearNotifications, getCurrentNotification } from '../shared/lib/notifications';

const {
  createFolderMock, deleteFoldersMock, moveFolderMock, renameFolderMock,
  reorderFolderChildrenMock, sortFolderItemsMock,
} = vi.hoisted(() => ({
  createFolderMock: vi.fn(),
  deleteFoldersMock: vi.fn(),
  moveFolderMock: vi.fn(),
  renameFolderMock: vi.fn(),
  reorderFolderChildrenMock: vi.fn(),
  sortFolderItemsMock: vi.fn(),
}));

vi.mock('../platform/folderApi', () => ({
  addMedia: vi.fn(),
  clearFolderWatchConfig: vi.fn(),
  createFolder: createFolderMock,
  deleteFolders: deleteFoldersMock,
  getFolderCoverHashes: vi.fn(),
  moveFolder: moveFolderMock,
  renameFolder: renameFolderMock,
  reorderFolderChildren: reorderFolderChildrenMock,
  setFolderMetadata: vi.fn(),
  setFolderWatchConfig: vi.fn(),
  sortFolderItems: sortFolderItemsMock,
}));

import {
  bulkFolderDeletionMessage,
  foldersController,
  singleFolderDeletionMessage,
} from './foldersController';

const store = getDefaultStore();

function deletionReceipt(deleted: number[], fallback: number | null) {
  return {
    receipt: { revision: 1, resources: ['folders', 'sidebar'], item_ids: [] },
    folder_ids: deleted,
    deleted_folder_ids: deleted,
    fallback_folder_id: fallback,
  };
}

describe('foldersController deletion settlement', () => {
  beforeEach(() => {
    deleteFoldersMock.mockReset();
    createFolderMock.mockReset();
    renameFolderMock.mockReset();
    moveFolderMock.mockReset();
    reorderFolderChildrenMock.mockReset();
    sortFolderItemsMock.mockReset();
    clearNotifications();
    store.set(activeNodeIdAtom, 'system:active');
    store.set(pendingSidebarRevealNodeIdAtom, null);
  });

  it('keeps the committed folder name visible while the backend settles', async () => {
    let finishRename!: () => void;
    renameFolderMock.mockImplementation(() => new Promise<void>((resolve) => { finishRename = resolve; }));
    store.set(sidebarNodesAtom, [{
      id: 'folder:7', kind: 'folder', parent_id: 'section:folders', name: 'Before',
      sort_order: 0, count: 0, freshness: 'exact', selectable: true,
    }]);

    const rename = foldersController.rename(7, 'After');
    expect(store.get(sidebarNodesAtom)[0]?.name).toBe('After');
    finishRename();
    await rename;
    expect(store.get(sidebarNodesAtom)[0]?.name).toBe('After');
  });

  it('requests that a newly created folder is revealed in the sidebar', async () => {
    createFolderMock.mockResolvedValue({ folder_id: 42 });

    await expect(foldersController.create('Child', 7)).resolves.toBe('folder:42');

    expect(createFolderMock).toHaveBeenCalledWith({ name: 'Child', parent_id: 7 });
    expect(store.get(pendingSidebarRevealNodeIdAtom)).toBe('folder:42');
  });

  it('warns when creating a folder would exceed the hierarchy limit', async () => {
    createFolderMock.mockRejectedValue(
      new Error('invalid input: folders may be nested at most 8 levels deep'),
    );

    await expect(foldersController.create('Too deep', 8)).rejects.toThrow('at most 8');

    expect(getCurrentNotification()).toMatchObject({
      tone: 'warning',
      title: 'Folder depth limit',
      message: expect.stringContaining('Choose a higher destination'),
    });
  });

  it('sorts folder contents by the requested field', async () => {
    sortFolderItemsMock.mockResolvedValue({ revision: 2, resources: [], item_ids: [] });

    await foldersController.sortContents(12, 'modified_at');

    expect(sortFolderItemsMock).toHaveBeenCalledWith(12, 'modified_at');
  });

  it('moves every selected root before reordering the destination once', async () => {
    moveFolderMock.mockResolvedValue({ revision: 1, resources: [], item_ids: [] });
    reorderFolderChildrenMock.mockResolvedValue({ revision: 2, resources: [], item_ids: [] });

    await foldersController.moveMany([2, 4], null, [1, 3, 2, 4, 5]);

    expect(moveFolderMock.mock.calls).toEqual([[2, null], [4, null]]);
    expect(reorderFolderChildrenMock).toHaveBeenCalledOnce();
    expect(reorderFolderChildrenMock).toHaveBeenCalledWith(null, [1, 3, 2, 4, 5]);
    expect(moveFolderMock.mock.invocationCallOrder[1]).toBeLessThan(
      reorderFolderChildrenMock.mock.invocationCallOrder[0],
    );
  });

  it('states recursive deletion and media preservation in confirmations', () => {
    expect(singleFolderDeletionMessage('Trips')).toContain('all its subfolders');
    expect(singleFolderDeletionMessage('Trips')).toContain('Media inside these folders will remain untouched');
    expect(bulkFolderDeletionMessage(2)).toContain('All selected folders and their subfolders will be deleted');
    expect(bulkFolderDeletionMessage(2)).toContain('Media inside these folders will remain untouched');
  });

  it('waits for backend success and uses its complete descendant receipt', async () => {
    let resolveDelete: ((value: unknown) => void) | undefined;
    deleteFoldersMock.mockImplementation(() => new Promise((resolve) => { resolveDelete = resolve; }));
    store.set(activeNodeIdAtom, 'folder:12');

    const deletion = foldersController.delete(10);
    expect(store.get(activeNodeIdAtom)).toBe('folder:12');

    resolveDelete?.(deletionReceipt([10, 11, 12], null));
    await deletion;

    expect(deleteFoldersMock).toHaveBeenCalledWith([10]);
    expect(store.get(activeNodeIdAtom)).toBe('system:active');
  });

  it('uses the backend-provided nearest surviving parent', async () => {
    deleteFoldersMock.mockResolvedValue(deletionReceipt([21, 22], 20));
    store.set(activeNodeIdAtom, 'folder:22');

    await foldersController.delete(21);

    expect(store.get(activeNodeIdAtom)).toBe('folder:20');
  });

  it('leaves navigation unchanged when the command fails', async () => {
    deleteFoldersMock.mockRejectedValue(new Error('delete failed'));
    store.set(activeNodeIdAtom, 'folder:31');

    await expect(foldersController.delete(30)).rejects.toThrow('delete failed');

    expect(store.get(activeNodeIdAtom)).toBe('folder:31');
  });

  it('sends a multi-folder deletion as one backend mutation', async () => {
    deleteFoldersMock.mockResolvedValue(deletionReceipt([10, 11, 20], null));
    store.set(activeNodeIdAtom, 'folder:11');

    await foldersController.deleteMany([10, 11, 20, 10]);

    expect(deleteFoldersMock).toHaveBeenCalledOnce();
    expect(deleteFoldersMock).toHaveBeenCalledWith([10, 11, 20]);
    expect(store.get(activeNodeIdAtom)).toBe('system:active');
  });
});
