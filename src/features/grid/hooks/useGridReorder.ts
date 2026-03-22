import { useCallback } from 'react';
import type { FolderReorderMove } from '../../../shared/types/api';
import { collectionsController } from '../../../controllers/collectionsController';
import { foldersController } from '../../../controllers/foldersController';
import type { GridRuntimeAction } from '../runtime';
import type { MasonryItem } from '../shared';

export function useGridReorder(args: {
  folderId?: number | null;
  collectionEntityId?: number | null;
  dispatch: React.Dispatch<GridRuntimeAction>;
  stateRef: { current: { images: MasonryItem[] } };
  requestReplace: () => Promise<void>;
  displayFolderId: number | null;
}) {
  const { folderId, collectionEntityId, dispatch, stateRef, requestReplace, displayFolderId } = args;

  const isReorderScope = !!displayFolderId || !!collectionEntityId;

  const handleReorder = useCallback((movedHashes: string[], targetIndex: number) => {
    if (!folderId && !collectionEntityId) return;
    const currentFolderId = folderId ?? null;
    const currentCollectionId = collectionEntityId ?? null;
    const prev = stateRef.current.images;

    const movedSet = new Set(movedHashes);
    const remaining = prev.filter((img) => !movedSet.has(img.hash));
    const movedItems = movedHashes
      .map((h) => prev.find((img) => img.hash === h))
      .filter((img): img is (typeof prev)[number] => img != null);

    const movedBefore = prev.slice(0, targetIndex).filter((img) => movedSet.has(img.hash)).length;
    const insertAt = Math.max(0, Math.min(remaining.length, targetIndex - movedBefore));

    const next = [...remaining];
    next.splice(insertAt, 0, ...movedItems);

    if (currentCollectionId != null) {
      dispatch({ type: 'SET_IMAGES', images: next });
      collectionsController.reorderMembers(currentCollectionId, next.map((img) => img.hash)).catch((err) => {
        console.error('Collection reorder failed, reloading collection:', err);
        void requestReplace();
      });
      return;
    }

    const moves: FolderReorderMove[] = [];
    for (let i = 0; i < movedItems.length; i++) {
      const pos = insertAt + i;
      if (i === 0) {
        if (pos > 0) {
          moves.push({ hash: movedItems[i].hash, after_hash: next[pos - 1].hash, before_hash: null });
        } else if (next.length > movedItems.length) {
          moves.push({ hash: movedItems[i].hash, after_hash: null, before_hash: next[movedItems.length].hash });
        }
      } else {
        moves.push({ hash: movedItems[i].hash, after_hash: movedItems[i - 1].hash, before_hash: null });
      }
    }

    dispatch({ type: 'SET_IMAGES', images: next });

    if (moves.length > 0) {
      foldersController.reorderItems(currentFolderId!, moves).catch((err) => {
        console.error('Reorder failed, reloading folder:', err);
        void requestReplace();
      });
    }
  }, [collectionEntityId, dispatch, folderId, requestReplace, stateRef]);

  return {
    isReorderScope,
    handleReorder,
  };
}
