import { useCallback } from 'react';
import { deepClone, registerUndoAction } from '../../../controllers/undoRedoController';
import { api } from '#desktop/api';
import { notifyError, notifyInfo } from '../../../shared/lib/notify';
import { FileController } from '../../../controllers/fileController';
import { FolderController } from '../../../controllers/folderController';
import { SelectionController } from '../../../controllers/selectionController';
import {
  deleteHashesWithLifecycleEffects,
  deleteSelectionWithLifecycleEffects,
  setFileStatusWithLifecycleEffects,
  setStatusSelectionWithLifecycleEffects,
} from '../../../domain/actions/fileLifecycleActions';
import type { GridRuntimeAction, GridRuntimeState } from '../runtime';
import {
  buildExplicitSelectionSpec,
  effectiveSelectedHashes as selectEffectiveHashes,
  virtualSelectionSpec as selectVirtualSpec,
} from '../runtime';
import type { GridQueryBroker } from '../queryBroker/GridQueryBroker';
import type { GridQueryKey } from '../queryBroker/gridQueryKey';

type StatusSnapshot = { hash: string; status: string };

interface UseGridMutationActionsArgs {
  stateRef: { current: GridRuntimeState };
  dispatch: React.Dispatch<GridRuntimeAction>;
  statusFilter?: string | null;
  folderId?: number | null;
  collectionEntityId?: number | null;
  broker: GridQueryBroker;
  queryKeyRef: { current: GridQueryKey };
}

interface GridMutationActions {
  handleDeleteSelected: () => void;
  handleRateSelected: (rating: number) => void;
  handleRestoreSelected: () => void;
  handleInboxAction: (hash: string, status: 'active' | 'trash') => void;
  handleRemoveFromFolder: () => void;
  handleRemoveFromCollection: () => void;
}

export function useGridMutationActions({
  stateRef,
  dispatch,
  statusFilter,
  folderId,
  collectionEntityId,
  broker,
  queryKeyRef,
}: UseGridMutationActionsArgs): GridMutationActions {
  const restoreStatusesByHash = useCallback(async (items: StatusSnapshot[]) => {
    const buckets = new Map<string, string[]>();
    for (const item of items) {
      const status = item.status || 'active';
      const bucket = buckets.get(status);
      if (bucket) bucket.push(item.hash);
      else buckets.set(status, [item.hash]);
    }
    for (const [status, hashes] of buckets.entries()) {
      await api.file.setStatusSelection(buildExplicitSelectionSpec(hashes), status);
    }
  }, []);

  const handleDeleteSelected = useCallback(() => {
    const { virtualAllSelection, selectedHashes, images } = stateRef.current;
    const inTrash = statusFilter === 'trash';

    if (virtualAllSelection) {
      const spec = selectVirtualSpec(stateRef.current)!;
      const specSnapshot = deepClone(spec);
      const undoStatus = statusFilter ?? 'active';
      dispatch({
        type: 'FILTER_IMAGES',
        predicate: (i) => virtualAllSelection.excludedHashes.has(i.hash),
      });
      dispatch({ type: 'CLEAR_SELECTION' });
      const promise = inTrash
        ? deleteSelectionWithLifecycleEffects(spec, { gridReload: () => broker.requestReplace(queryKeyRef.current) })
        : setStatusSelectionWithLifecycleEffects(spec, 'trash', { gridReload: () => broker.requestReplace(queryKeyRef.current) });
      promise
        .then((count) => {
          if (!inTrash) {
            registerUndoAction({
              label: `Move ${count.toLocaleString()} image${count === 1 ? '' : 's'} to trash`,
              undo: async () => {
                await api.file.setStatusSelection(specSnapshot, undoStatus);
                broker.requestReplace(queryKeyRef.current);
              },
              redo: async () => {
                await api.file.setStatusSelection(specSnapshot, 'trash');
                broker.requestReplace(queryKeyRef.current);
              },
            });
          }
          notifyInfo(
            `${count.toLocaleString()} image${count === 1 ? '' : 's'} ${
              inTrash ? 'permanently deleted' : 'moved to trash'
            }`,
            inTrash ? 'Deleted' : 'Moved to Trash',
          );
        })
        .catch((err) => {
          notifyError(err, 'Delete Failed');
        });
      return;
    }

    if (selectedHashes.size === 0) return;
    const hashes = Array.from(selectedHashes);
    const previousStatuses = hashes.map((hash) => ({
      hash,
      status: images.find((img) => img.hash === hash)?.status ?? (statusFilter ?? 'active'),
    }));
    const explicitSpec = buildExplicitSelectionSpec(hashes);
    const hashSet = new Set(hashes);
    dispatch({ type: 'FILTER_IMAGES', predicate: (i) => !hashSet.has(i.hash) });
    dispatch({ type: 'CLEAR_SELECTION' });

    if (inTrash) {
      deleteHashesWithLifecycleEffects(hashes, { gridReload: () => broker.requestReplace(queryKeyRef.current) })
        .catch((err) => {
          notifyError(err, 'Delete Failed');
        });
    } else {
      setStatusSelectionWithLifecycleEffects(explicitSpec, 'trash', { gridReload: () => broker.requestReplace(queryKeyRef.current) })
        .then(() => {
          registerUndoAction({
            label: `Move ${hashes.length.toLocaleString()} image${
              hashes.length === 1 ? '' : 's'
            } to trash`,
            undo: async () => {
              await restoreStatusesByHash(previousStatuses);
              broker.requestReplace(queryKeyRef.current);
            },
            redo: async () => {
              await api.file.setStatusSelection(explicitSpec, 'trash');
              broker.requestReplace(queryKeyRef.current);
            },
          });
        })
        .catch((err) => {
          notifyError(err, 'Delete Failed');
        });
    }
  }, [stateRef, statusFilter, dispatch, restoreStatusesByHash, broker, queryKeyRef]);

  const handleRateSelected = useCallback(
    (rating: number) => {
      const { virtualAllSelection, selectedHashes } = stateRef.current;
      const normalizedRating = rating || null;
      if (virtualAllSelection) {
        const spec = selectVirtualSpec(stateRef.current)!;
        SelectionController.updateRatingSelection(spec, normalizedRating).catch((err) =>
          notifyError(err, 'Rating Failed'),
        );
      } else {
        const hashes = [...selectedHashes];
        if (hashes.length === 0) return;
        Promise.all(hashes.map((hash) => FileController.updateRating(hash, rating))).catch((err) =>
          notifyError(err, 'Rating Failed'),
        );
      }
    },
    [stateRef],
  );

  const handleRestoreSelected = useCallback(() => {
    const { virtualAllSelection, selectedHashes } = stateRef.current;
    if (virtualAllSelection) {
      const spec = selectVirtualSpec(stateRef.current)!;
      const specSnapshot = deepClone(spec);
      dispatch({
        type: 'FILTER_IMAGES',
        predicate: (i) => virtualAllSelection.excludedHashes.has(i.hash),
      });
      dispatch({ type: 'CLEAR_SELECTION' });
      setStatusSelectionWithLifecycleEffects(spec, 'active', { gridReload: () => broker.requestReplace(queryKeyRef.current) })
        .then((count) => {
          registerUndoAction({
            label: `Restore ${count.toLocaleString()} image${count === 1 ? '' : 's'}`,
            undo: async () => {
              await api.file.setStatusSelection(specSnapshot, 'trash');
              broker.requestReplace(queryKeyRef.current);
            },
            redo: async () => {
              await api.file.setStatusSelection(specSnapshot, 'active');
              broker.requestReplace(queryKeyRef.current);
            },
          });
          notifyInfo(`${count.toLocaleString()} image${count === 1 ? '' : 's'} restored`, 'Restored');
        })
        .catch((err) => {
          notifyError(err, 'Restore Failed');
        });
      return;
    }

    if (selectedHashes.size === 0) return;
    const hashes = Array.from(selectedHashes);
    const explicitSpec = buildExplicitSelectionSpec(hashes);
    const hashSet = new Set(hashes);
    dispatch({ type: 'FILTER_IMAGES', predicate: (i) => !hashSet.has(i.hash) });
    dispatch({ type: 'CLEAR_SELECTION' });
    setStatusSelectionWithLifecycleEffects(explicitSpec, 'active', { gridReload: () => broker.requestReplace(queryKeyRef.current) })
      .then((count) => {
        registerUndoAction({
          label: `Restore ${count.toLocaleString()} image${count === 1 ? '' : 's'}`,
          undo: async () => {
            await api.file.setStatusSelection(explicitSpec, 'trash');
            broker.requestReplace(queryKeyRef.current);
          },
          redo: async () => {
            await api.file.setStatusSelection(explicitSpec, 'active');
            broker.requestReplace(queryKeyRef.current);
          },
        });
        notifyInfo(`${count.toLocaleString()} image${count === 1 ? '' : 's'} restored`, 'Restored');
      })
      .catch((err) => {
        notifyError(err, 'Restore Failed');
      });
  }, [stateRef, dispatch, broker, queryKeyRef]);

  const handleInboxAction = useCallback(
    (hash: string, status: 'active' | 'trash') => {
      setFileStatusWithLifecycleEffects(hash, status, { gridReload: () => broker.requestReplace(queryKeyRef.current) })
        .then(() => {
          registerUndoAction({
            label: status === 'active' ? 'Accept inbox image' : 'Reject inbox image',
            undo: async () => {
              await api.file.setStatus(hash, 'inbox');
              broker.requestReplace(queryKeyRef.current);
            },
            redo: async () => {
              await api.file.setStatus(hash, status);
              broker.requestReplace(queryKeyRef.current);
            },
          });
        })
        .catch((err) => {
          notifyError(err, status === 'active' ? 'Accept Failed' : 'Reject Failed');
        });
      dispatch({ type: 'FILTER_IMAGES', predicate: (i) => i.hash !== hash });
      dispatch({ type: 'REMOVE_HASHES', hashes: new Set([hash]) });
    },
    [dispatch, broker, queryKeyRef],
  );

  const handleRemoveFromFolder = useCallback(() => {
    if (!folderId) return;
    const effective = selectEffectiveHashes(stateRef.current);
    const hashes = [...effective];
    if (hashes.length === 0) return;
    dispatch({ type: 'FILTER_IMAGES', predicate: (i) => !effective.has(i.hash) });
    dispatch({ type: 'CLEAR_SELECTION' });
    FolderController.removeFilesFromFolderBatch(folderId, hashes)
      .then(() => {
        registerUndoAction({
          label: `Remove ${hashes.length} from folder`,
          undo: async () => {
            await FolderController.addFilesToFolderBatch(folderId, hashes);
            broker.requestReplace(queryKeyRef.current);
          },
          redo: async () => {
            await FolderController.removeFilesFromFolderBatch(folderId, hashes);
            broker.requestReplace(queryKeyRef.current);
          },
        });
      })
      .catch((err) => {
        notifyError(err, 'Remove from Folder Failed');
      });
  }, [folderId, stateRef, dispatch, broker, queryKeyRef]);

  const handleRemoveFromCollection = useCallback(() => {
    if (!collectionEntityId) return;
    const effective = selectEffectiveHashes(stateRef.current);
    const hashes = [...effective];
    if (hashes.length === 0) return;
    dispatch({ type: 'FILTER_IMAGES', predicate: (i) => !effective.has(i.hash) });
    dispatch({ type: 'CLEAR_SELECTION' });
    api.collections.removeMembers({ id: collectionEntityId, hashes })
      .then(() => {
        registerUndoAction({
          label: `Remove ${hashes.length} from collection`,
          undo: async () => {
            await api.collections.addMembers({ id: collectionEntityId, hashes });
            broker.requestReplace(queryKeyRef.current);
          },
          redo: async () => {
            await api.collections.removeMembers({ id: collectionEntityId, hashes });
            broker.requestReplace(queryKeyRef.current);
          },
        });
      })
      .catch((err) => {
        notifyError(err, 'Remove from Collection Failed');
      });
  }, [collectionEntityId, stateRef, dispatch, broker, queryKeyRef]);

  return {
    handleDeleteSelected,
    handleRateSelected,
    handleRestoreSelected,
    handleInboxAction,
    handleRemoveFromFolder,
    handleRemoveFromCollection,
  };
}
