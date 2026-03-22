import { useCallback } from 'react';
import { entityController } from '../../../controllers/entityController';
import { foldersController } from '../../../controllers/foldersController';
import { collectionsController } from '../../../controllers/collectionsController';
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

    // Build spec BEFORE clearing selection
    if (inTrash) {
      const spec = virtualAllSelection
        ? selectVirtualSpec(state)!
        : buildExplicitSelectionSpec([...selectedHashes]);
      dispatch({ type: 'CLEAR_SELECTION' });
      entityController.permanentDeleteSelection(spec)
        .then((count) => notifyInfo(`${plural(count)} permanently deleted`, 'Deleted'))
        .catch((err) => notifyError(err, 'Delete Failed'));
      return;
    }

    if (virtualAllSelection) {
      const spec = selectVirtualSpec(state)!;
      dispatch({ type: 'CLEAR_SELECTION' });
      entityController.trashSelection(spec)
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
      dispatch({ type: 'CLEAR_SELECTION' });
      entityController.trashSelection(spec, previousStatuses)
        .then((n) => notifyInfo(`Moved ${plural(n)} to trash`, 'Moved to Trash'))
        .catch((err) => notifyError(err, 'Delete Failed'));
    }
  }, [stateRef, statusFilter, dispatch]);

  const handleRateSelected = useCallback((rating: number) => {
    const { virtualAllSelection, selectedHashes, images } = stateRef.current;
    const normalizedRating = rating || null;

    if (virtualAllSelection) {
      const spec = selectVirtualSpec(stateRef.current)!;
      entityController.rateSelection(spec, normalizedRating)
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
    entityController.rateSelection(spec, normalizedRating, prevRatings)
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

    entityController.restoreSelection(spec)
      .then((n) => notifyInfo(`Restored ${plural(n)}`, 'Restored'))
      .catch((err) => notifyError(err, 'Restore Failed'));
  }, [stateRef, dispatch]);

  const handleInboxAction = useCallback((hash: string, status: 'active' | 'trash') => {
    entityController.inboxAction(hash, status)
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

    entityController.inboxSelectionAction(spec, status)
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
      .catch((err) => notifyError(err, 'Remove from Folder Failed'));
  }, [folderId, stateRef, dispatch]);

  const handleRemoveFromCollection = useCallback(() => {
    if (!collectionEntityId) return;
    const effective = selectEffectiveHashes(stateRef.current);
    const hashes = [...effective];
    if (hashes.length === 0) return;
    dispatch({ type: 'CLEAR_SELECTION' });
    collectionsController.removeMembersBatch(collectionEntityId, hashes)
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
