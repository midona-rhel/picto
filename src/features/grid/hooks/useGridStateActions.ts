import { useCallback } from 'react';
import { filesController } from '../../../controllers/filesController';
import { foldersController } from '../../../controllers/foldersController';
import { collectionsController } from '../../../controllers/collectionsController';
import { registerUndoAction } from '../../../shared/controllers/undoRedoController';
import { notifyError, notifyInfo } from '../../../shared/lib/notify';
import type { GridRuntimeAction, GridRuntimeState } from '../runtime';
import {
  buildExplicitSelectionSpec,
  effectiveSelectedHashes as selectEffectiveHashes,
  virtualSelectionSpec as selectVirtualSpec,
} from '../runtime';

const plural = (n: number) => `${n.toLocaleString()} item${n === 1 ? '' : 's'}`;

interface UseGridStateActionsArgs {
  stateRef: { current: GridRuntimeState };
  dispatch: React.Dispatch<GridRuntimeAction>;
  statusFilter?: string | null;
  folderId?: number | null;
  collectionEntityId?: number | null;
}

interface GridStateActions {
  handleDeleteSelected: () => void;
  handleRateSelected: (rating: number) => void;
  handleRestoreSelected: () => void;
  handleInboxAction: (hash: string, status: 'active' | 'trash') => void;
  handleInboxSelectionAction: (status: 'active' | 'trash') => void;
  handleRemoveFromFolder: () => void;
  handleRemoveFromCollection: () => void;
}

export function useGridStateActions({
  stateRef,
  dispatch,
  statusFilter,
  folderId,
  collectionEntityId,
}: UseGridStateActionsArgs): GridStateActions {

  const handleDeleteSelected = useCallback(() => {
    const state = stateRef.current;
    const { virtualAllSelection, selectedHashes, images } = state;
    const inTrash = statusFilter === 'trash';

    dispatch({ type: 'CLEAR_SELECTION' });

    if (inTrash) {
      // Permanent delete — no undo
      const spec = virtualAllSelection
        ? selectVirtualSpec(state)!
        : buildExplicitSelectionSpec([...selectedHashes]);
      filesController.permanentDeleteSelection(spec)
        .then((count) => notifyInfo(`${plural(count)} permanently deleted`, 'Deleted'))
        .catch((err) => notifyError(err, 'Delete Failed'));
      return;
    }

    // Move to trash — controller owns undo
    if (virtualAllSelection) {
      const spec = selectVirtualSpec(state)!;
      filesController.trashSelection(spec)
        .then((n) => notifyInfo(`Moved ${plural(n)} to trash`, 'Moved to Trash'))
        .catch((err) => notifyError(err, 'Delete Failed'));
    } else {
      if (selectedHashes.size === 0) return;
      const hashes = [...selectedHashes];
      const previousStatuses = hashes.map((hash) => ({
        hash,
        status: images.find((img) => img.hash === hash)?.status ?? (statusFilter ?? 'active'),
      }));
      const spec = buildExplicitSelectionSpec(hashes);
      filesController.trashSelection(spec, previousStatuses)
        .then((n) => notifyInfo(`Moved ${plural(n)} to trash`, 'Moved to Trash'))
        .catch((err) => notifyError(err, 'Delete Failed'));
    }
  }, [stateRef, statusFilter, dispatch]);

  const handleRateSelected = useCallback((rating: number) => {
    const { virtualAllSelection, selectedHashes, images } = stateRef.current;
    const normalizedRating = rating || null;

    if (virtualAllSelection) {
      const spec = selectVirtualSpec(stateRef.current)!;
      filesController.rateSelection(spec, normalizedRating)
        .catch((err) => notifyError(err, 'Rating Failed'));
      return;
    }

    const hashes = [...selectedHashes];
    if (hashes.length === 0) return;
    const prevRatings = new Map<string, number | null>();
    for (const hash of hashes) {
      const img = images.find((i) => i.hash === hash);
      prevRatings.set(hash, img?.rating ?? null);
    }
    const spec = buildExplicitSelectionSpec(hashes);
    filesController.rateSelection(spec, normalizedRating, prevRatings)
      .catch((err) => notifyError(err, 'Rating Failed'));
  }, [stateRef]);

  const handleRestoreSelected = useCallback(() => {
    const state = stateRef.current;
    const { virtualAllSelection, selectedHashes } = state;
    if (!virtualAllSelection && selectedHashes.size === 0) return;

    dispatch({ type: 'CLEAR_SELECTION' });

    const spec = virtualAllSelection
      ? selectVirtualSpec(state)!
      : buildExplicitSelectionSpec([...selectedHashes]);

    filesController.restoreSelection(spec)
      .then((n) => notifyInfo(`Restored ${plural(n)}`, 'Restored'))
      .catch((err) => notifyError(err, 'Restore Failed'));
  }, [stateRef, dispatch]);

  const handleInboxAction = useCallback((hash: string, status: 'active' | 'trash') => {
    filesController.inboxAction(hash, status)
      .catch((err) => notifyError(err, status === 'active' ? 'Accept Failed' : 'Reject Failed'));
  }, []);

  const handleInboxSelectionAction = useCallback((status: 'active' | 'trash') => {
    const state = stateRef.current;
    const { virtualAllSelection, selectedHashes } = state;
    if (!virtualAllSelection && selectedHashes.size === 0) return;

    dispatch({ type: 'CLEAR_SELECTION' });

    const spec = virtualAllSelection
      ? selectVirtualSpec(state)!
      : buildExplicitSelectionSpec([...selectedHashes]);
    const past = status === 'active' ? 'Accepted' : 'Rejected';

    filesController.inboxSelectionAction(spec, status)
      .then((n) => notifyInfo(`${past} ${plural(n)}`, past))
      .catch((err) => notifyError(err, `${status === 'active' ? 'Accept' : 'Reject'} Failed`));
  }, [dispatch, stateRef]);

  const handleRemoveFromFolder = useCallback(() => {
    if (!folderId) return;
    const effective = selectEffectiveHashes(stateRef.current);
    const hashes = [...effective];
    if (hashes.length === 0) return;
    dispatch({ type: 'CLEAR_SELECTION' });
    foldersController.removeFiles(folderId, hashes)
      .then(() => {
        registerUndoAction({
          label: `Remove ${hashes.length} from folder`,
          undo: async () => { await foldersController.addFiles(folderId, hashes); },
          redo: async () => { await foldersController.removeFiles(folderId, hashes); },
        });
      })
      .catch((err) => notifyError(err, 'Remove from Folder Failed'));
  }, [folderId, stateRef, dispatch]);

  const handleRemoveFromCollection = useCallback(() => {
    if (!collectionEntityId) return;
    const effective = selectEffectiveHashes(stateRef.current);
    const hashes = [...effective];
    if (hashes.length === 0) return;
    dispatch({ type: 'CLEAR_SELECTION' });
    collectionsController.removeMembers({ id: collectionEntityId, hashes })
      .then(() => {
        registerUndoAction({
          label: `Remove ${hashes.length} from collection`,
          undo: async () => { await collectionsController.addMembers({ id: collectionEntityId, hashes }); },
          redo: async () => { await collectionsController.removeMembers({ id: collectionEntityId, hashes }); },
        });
      })
      .catch((err) => notifyError(err, 'Remove from Collection Failed'));
  }, [collectionEntityId, stateRef, dispatch]);

  return {
    handleDeleteSelected,
    handleRateSelected,
    handleRestoreSelected,
    handleInboxAction,
    handleInboxSelectionAction,
    handleRemoveFromFolder,
    handleRemoveFromCollection,
  };
}
