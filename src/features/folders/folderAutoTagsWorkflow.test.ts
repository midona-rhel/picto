import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import { foldersController } from '../../controllers/foldersController';
import { clearNotifications, getCurrentNotification } from '../../shared/lib/notifications';
import { folderAutoTagsModalAtom } from '../../state/modals';
import { openFolderAutoTagsEditor } from './folderAutoTagsWorkflow';

describe('folder auto tags workflow', () => {
  const store = getDefaultStore();

  beforeEach(() => {
    vi.restoreAllMocks();
    clearNotifications();
    store.set(folderAutoTagsModalAtom, {
      open: false,
      folderIds: [],
      folderName: null,
      initialTags: [],
    });
  });

  it('opens one editor with tags common to every selected folder', async () => {
    vi.spyOn(foldersController, 'getAutoTags')
      .mockResolvedValueOnce(['creator:alice', 'rating:safe'])
      .mockResolvedValueOnce(['rating:safe', 'series:test']);

    await openFolderAutoTagsEditor([9, 9, 12], 'Ignored for bulk');

    expect(foldersController.getAutoTags).toHaveBeenCalledTimes(2);
    expect(store.get(folderAutoTagsModalAtom)).toEqual({
      open: true,
      folderIds: [9, 12],
      folderName: null,
      initialTags: ['rating:safe'],
    });
  });

  it('uses the notification path and leaves the modal closed when loading fails', async () => {
    vi.spyOn(foldersController, 'getAutoTags').mockRejectedValue(new Error('database unavailable'));

    await openFolderAutoTagsEditor([7], 'References');

    expect(store.get(folderAutoTagsModalAtom).open).toBe(false);
    expect(getCurrentNotification()).toMatchObject({
      tone: 'error',
      title: 'Could not load folder auto tags',
      message: 'database unavailable',
    });
  });
});
