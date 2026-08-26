import { getDefaultStore } from 'jotai';
import { foldersController } from '../../controllers/foldersController';
import { showErrorNotification } from '../../shared/lib/notifications';
import { folderAutoTagsModalAtom } from '../../state/modals';

const store = getDefaultStore();

export async function openFolderAutoTagsEditor(
  folderIds: number[],
  folderName: string | null,
): Promise<void> {
  const uniqueIds = [...new Set(folderIds)];
  if (uniqueIds.length === 0) return;

  try {
    const configured = await Promise.all(
      uniqueIds.map((folderId) => foldersController.getAutoTags(folderId)),
    );
    const commonTags = configured.slice(1).reduce(
      (common, current) => common.filter((tag) => current.includes(tag)),
      configured[0] ?? [],
    );
    store.set(folderAutoTagsModalAtom, {
      open: true,
      folderIds: uniqueIds,
      folderName: uniqueIds.length === 1 ? folderName : null,
      initialTags: commonTags,
    });
  } catch (reason) {
    showErrorNotification({
      title: 'Could not load folder auto tags',
      message: reason instanceof Error ? reason.message : String(reason),
    });
  }
}
