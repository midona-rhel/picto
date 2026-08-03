import { getDefaultStore } from 'jotai';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { SidebarNodeDto } from '../shared/types/canonical';
import { activeNodeIdAtom } from '../state/navigation';
import { getScrollPosition, goBack, goForward, pushHistory, saveScrollPosition } from '../state/navigationHistory';
import { sidebarNodesAtom } from '../state/sidebar';

const { deleteFolderMock } = vi.hoisted(() => ({ deleteFolderMock: vi.fn() }));

vi.mock('../platform/folderApi', () => ({
  addMedia: vi.fn(),
  clearFolderWatchConfig: vi.fn(),
  createFolder: vi.fn(),
  deleteFolder: deleteFolderMock,
  getFolderCoverHash: vi.fn(),
  moveFolder: vi.fn(),
  renameFolder: vi.fn(),
  reorderFolderItems: vi.fn(),
  setFolderWatchConfig: vi.fn(),
  updateFolder: vi.fn(),
}));

import {
  bulkFolderDeletionMessage,
  foldersController,
  singleFolderDeletionMessage,
} from './foldersController';

const store = getDefaultStore();

function folder(id: number, parentId: number | null): SidebarNodeDto {
  return {
    id: `folder:${id}`,
    kind: 'folder',
    parent_id: parentId == null ? 'section:folders' : `folder:${parentId}`,
    name: `Folder ${id}`,
    count: 0,
    freshness: 'exact',
    selectable: true,
  };
}

describe('foldersController deletion settlement', () => {
  beforeEach(() => {
    deleteFolderMock.mockReset();
    store.set(sidebarNodesAtom, []);
    store.set(activeNodeIdAtom, 'system:active');
  });

  it('states recursive deletion and media preservation in confirmations', () => {
    expect(singleFolderDeletionMessage('Trips')).toContain('all its subfolders');
    expect(singleFolderDeletionMessage('Trips')).toContain('Media inside these folders will remain untouched');
    expect(bulkFolderDeletionMessage(2)).toContain('All selected folders and their subfolders will be deleted');
    expect(bulkFolderDeletionMessage(2)).toContain('Media inside these folders will remain untouched');
  });

  it('waits for backend success before removing a recursive hierarchy', async () => {
    let resolveDelete: (() => void) | undefined;
    deleteFolderMock.mockImplementation(() => new Promise<void>((resolve) => { resolveDelete = resolve; }));
    store.set(sidebarNodesAtom, [folder(10, null), folder(11, 10), folder(12, 11), folder(13, null)]);
    store.set(activeNodeIdAtom, 'folder:12');

    const deletion = foldersController.delete(10);
    expect(store.get(sidebarNodesAtom).map((node) => node.id)).toEqual(['folder:10', 'folder:11', 'folder:12', 'folder:13']);
    expect(store.get(activeNodeIdAtom)).toBe('folder:12');

    resolveDelete?.();
    await deletion;

    expect(deleteFolderMock).toHaveBeenCalledWith(10);
    expect(store.get(sidebarNodesAtom).map((node) => node.id)).toEqual(['folder:13']);
    expect(store.get(activeNodeIdAtom)).toBe('system:active');
  });

  it('falls back to the nearest surviving parent and prunes deleted navigation state', async () => {
    deleteFolderMock.mockResolvedValue(undefined);
    store.set(sidebarNodesAtom, [folder(20, null), folder(21, 20), folder(22, 21), folder(23, null)]);
    store.set(activeNodeIdAtom, 'folder:20');
    pushHistory('folder:20');
    store.set(activeNodeIdAtom, 'folder:21');
    pushHistory('folder:21');
    store.set(activeNodeIdAtom, 'folder:22');
    pushHistory('folder:22');
    saveScrollPosition('folder:21', 120);
    saveScrollPosition('folder:22', 240);

    await foldersController.delete(21);

    expect(store.get(activeNodeIdAtom)).toBe('folder:20');
    expect(getScrollPosition('folder:21')).toBeNull();
    expect(getScrollPosition('folder:22')).toBeNull();

    store.set(activeNodeIdAtom, 'folder:23');
    pushHistory('folder:23');
    await goBack();
    expect(store.get(activeNodeIdAtom)).toBe('folder:20');
    await goForward();
    expect(store.get(activeNodeIdAtom)).toBe('folder:23');
  });

  it('leaves sidebar and navigation unchanged when the command fails', async () => {
    deleteFolderMock.mockRejectedValue(new Error('delete failed'));
    store.set(sidebarNodesAtom, [folder(30, null), folder(31, 30)]);
    store.set(activeNodeIdAtom, 'folder:31');

    await expect(foldersController.delete(30)).rejects.toThrow('delete failed');

    expect(store.get(sidebarNodesAtom).map((node) => node.id)).toEqual(['folder:30', 'folder:31']);
    expect(store.get(activeNodeIdAtom)).toBe('folder:31');
  });
});
