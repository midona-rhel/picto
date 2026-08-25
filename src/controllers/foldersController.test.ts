import { getDefaultStore } from 'jotai';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { activeNodeIdAtom } from '../state/navigation';

const { deleteFoldersMock } = vi.hoisted(() => ({ deleteFoldersMock: vi.fn() }));

vi.mock('../platform/folderApi', () => ({
  addMedia: vi.fn(),
  clearFolderWatchConfig: vi.fn(),
  createFolder: vi.fn(),
  deleteFolders: deleteFoldersMock,
  getFolderCoverHashes: vi.fn(),
  moveFolder: vi.fn(),
  renameFolder: vi.fn(),
  reorderFolderChildren: vi.fn(),
  setFolderMetadata: vi.fn(),
  setFolderWatchConfig: vi.fn(),
  sortFolderItemsByName: vi.fn(),
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
    store.set(activeNodeIdAtom, 'system:active');
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
