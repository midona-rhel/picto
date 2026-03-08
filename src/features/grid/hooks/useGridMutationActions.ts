import { useCallback } from 'react';
import { registerUndoAction } from '../../../shared/controllers/undoRedoController';
import { api } from '#desktop/api';
import { notifyError, notifyInfo } from '../../../shared/lib/notify';
import type { GridRuntimeAction, GridRuntimeState } from '../runtime';
import {
  buildExplicitSelectionSpec,
  effectiveSelectedHashes as selectEffectiveHashes,
  virtualSelectionSpec as selectVirtualSpec,
} from '../runtime';

type StatusSnapshot = { hash: string; status: string };

interface UseGridMutationActionsArgs {
  stateRef: { current: GridRuntimeState };
  dispatch: React.Dispatch<GridRuntimeAction>;
  statusFilter?: string | null;
  folderId?: number | null;
  collectionEntityId?: number | null;
  requestGridReload: () => void;
}

interface GridMutationActions {
  handleDeleteSelected: () => void;
  handleRateSelected: (rating: number) => void;
  handleRestoreSelected: () => void;
  handleInboxAction: (hash: string, status: 'active' | 'trash') => void;
  handleInboxSelectionAction: (status: 'active' | 'trash') => void;
  handleRemoveFromFolder: () => void;
  handleRemoveFromCollection: () => void;
}

export function useGridMutationActions({
  stateRef,
  dispatch,
  statusFilter,
  folderId,
  collectionEntityId,
  requestGridReload,
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
      const specSnapshot = structuredClone(spec);
      const undoStatus = statusFilter ?? 'active';
      dispatch({
        type: 'FILTER_IMAGES',
        predicate: (i) => virtualAllSelection.excludedHashes.has(i.hash),
      });
      dispatch({ type: 'CLEAR_SELECTION' });
      const promise = inTrash
        ? api.file.deleteSelection(spec)
        : api.file.setStatusSelection(spec, 'trash');
      promise
        .then((count) => {
          const affectedCount = Number(count ?? 0);
          if (!inTrash) {
            registerUndoAction({
              label: `Move ${affectedCount.toLocaleString()} image${affectedCount === 1 ? '' : 's'} to trash`,
              undo: async () => {
                await api.file.setStatusSelection(specSnapshot, undoStatus);
                requestGridReload();
              },
              redo: async () => {
                await api.file.setStatusSelection(specSnapshot, 'trash');
                requestGridReload();
              },
            });
          }
          notifyInfo(
            `${affectedCount.toLocaleString()} image${affectedCount === 1 ? '' : 's'} ${
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
      api.file.deleteMany(hashes).catch((err) => {
        notifyError(err, 'Delete Failed');
      });
      return;
    }

    api.file.setStatusSelection(explicitSpec, 'trash')
      .then(() => {
        registerUndoAction({
          label: `Move ${hashes.length.toLocaleString()} image${hashes.length === 1 ? '' : 's'} to trash`,
          undo: async () => {
            await restoreStatusesByHash(previousStatuses);
            requestGridReload();
          },
          redo: async () => {
            await api.file.setStatusSelection(explicitSpec, 'trash');
            requestGridReload();
          },
        });
      })
      .catch((err) => {
        notifyError(err, 'Delete Failed');
      });
  }, [stateRef, statusFilter, dispatch, restoreStatusesByHash, requestGridReload]);

  const handleRateSelected = useCallback((rating: number) => {
    const { virtualAllSelection, selectedHashes } = stateRef.current;
    const normalizedRating = rating || null;
    if (virtualAllSelection) {
      const spec = selectVirtualSpec(stateRef.current)!;
      api.selection.updateRating(spec, normalizedRating).catch((err) =>
        notifyError(err, 'Rating Failed'),
      );
      return;
    }

    const hashes = [...selectedHashes];
    if (hashes.length === 0) return;
    Promise.all(hashes.map((hash) => api.file.updateRating(hash, rating))).catch((err) =>
      notifyError(err, 'Rating Failed'),
    );
  }, [stateRef]);

  const handleRestoreSelected = useCallback(() => {
    const { virtualAllSelection, selectedHashes } = stateRef.current;
    if (virtualAllSelection) {
      const spec = selectVirtualSpec(stateRef.current)!;
      const specSnapshot = structuredClone(spec);
      dispatch({
        type: 'FILTER_IMAGES',
        predicate: (i) => virtualAllSelection.excludedHashes.has(i.hash),
      });
      dispatch({ type: 'CLEAR_SELECTION' });
      api.file.setStatusSelection(spec, 'active')
        .then((count) => {
          const affectedCount = Number(count ?? 0);
          registerUndoAction({
            label: `Restore ${affectedCount.toLocaleString()} image${affectedCount === 1 ? '' : 's'}`,
            undo: async () => {
              await api.file.setStatusSelection(specSnapshot, 'trash');
              requestGridReload();
            },
            redo: async () => {
              await api.file.setStatusSelection(specSnapshot, 'active');
              requestGridReload();
            },
          });
          notifyInfo(`${affectedCount.toLocaleString()} image${affectedCount === 1 ? '' : 's'} restored`, 'Restored');
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
    api.file.setStatusSelection(explicitSpec, 'active')
      .then((count) => {
        const affectedCount = Number(count ?? 0);
        registerUndoAction({
          label: `Restore ${affectedCount.toLocaleString()} image${affectedCount === 1 ? '' : 's'}`,
          undo: async () => {
            await api.file.setStatusSelection(explicitSpec, 'trash');
            requestGridReload();
          },
          redo: async () => {
            await api.file.setStatusSelection(explicitSpec, 'active');
            requestGridReload();
          },
        });
        notifyInfo(`${affectedCount.toLocaleString()} image${affectedCount === 1 ? '' : 's'} restored`, 'Restored');
      })
      .catch((err) => {
        notifyError(err, 'Restore Failed');
      });
  }, [stateRef, dispatch, requestGridReload]);

  const handleInboxAction = useCallback((hash: string, status: 'active' | 'trash') => {
    api.file.setStatus(hash, status)
      .then(() => {
        registerUndoAction({
          label: status === 'active' ? 'Accept inbox image' : 'Reject inbox image',
          undo: async () => {
            await api.file.setStatus(hash, 'inbox');
            requestGridReload();
          },
          redo: async () => {
            await api.file.setStatus(hash, status);
            requestGridReload();
          },
        });
      })
      .catch((err) => {
        notifyError(err, status === 'active' ? 'Accept Failed' : 'Reject Failed');
      });
    dispatch({ type: 'FILTER_IMAGES', predicate: (i) => i.hash !== hash });
    dispatch({ type: 'REMOVE_HASHES', hashes: new Set([hash]) });
  }, [dispatch, requestGridReload]);

  const handleInboxSelectionAction = useCallback((status: 'active' | 'trash') => {
    const { virtualAllSelection, selectedHashes } = stateRef.current;
    const actionVerb = status === 'active' ? 'Accept' : 'Reject';
    const actionPast = status === 'active' ? 'Accepted' : 'Rejected';

    if (virtualAllSelection) {
      const spec = selectVirtualSpec(stateRef.current);
      if (!spec) return;
      const specSnapshot = structuredClone(spec);
      dispatch({
        type: 'FILTER_IMAGES',
        predicate: (i) => virtualAllSelection.excludedHashes.has(i.hash),
      });
      dispatch({ type: 'CLEAR_SELECTION' });
      api.file.setStatusSelection(spec, status)
        .then((count) => {
          const affectedCount = Number(count ?? 0);
          registerUndoAction({
            label: `${actionVerb} ${affectedCount.toLocaleString()} inbox image${affectedCount === 1 ? '' : 's'}`,
            undo: async () => {
              await api.file.setStatusSelection(specSnapshot, 'inbox');
              requestGridReload();
            },
            redo: async () => {
              await api.file.setStatusSelection(specSnapshot, status);
              requestGridReload();
            },
          });
          notifyInfo(
            `${actionPast} ${affectedCount.toLocaleString()} image${affectedCount === 1 ? '' : 's'}`,
            actionPast,
          );
        })
        .catch((err) => {
          notifyError(err, status === 'active' ? 'Accept Failed' : 'Reject Failed');
        });
      return;
    }

    const hashes = Array.from(selectedHashes);
    if (hashes.length === 0) return;
    const explicitSpec = buildExplicitSelectionSpec(hashes);
    const hashSet = new Set(hashes);
    dispatch({ type: 'FILTER_IMAGES', predicate: (i) => !hashSet.has(i.hash) });
    dispatch({ type: 'CLEAR_SELECTION' });
    api.file.setStatusSelection(explicitSpec, status)
      .then((count) => {
        const affectedCount = Number(count ?? hashes.length);
        registerUndoAction({
          label: `${actionVerb} ${affectedCount.toLocaleString()} inbox image${affectedCount === 1 ? '' : 's'}`,
          undo: async () => {
            await api.file.setStatusSelection(explicitSpec, 'inbox');
            requestGridReload();
          },
          redo: async () => {
            await api.file.setStatusSelection(explicitSpec, status);
            requestGridReload();
          },
        });
        notifyInfo(
          `${actionPast} ${affectedCount.toLocaleString()} image${affectedCount === 1 ? '' : 's'}`,
          actionPast,
        );
      })
      .catch((err) => {
        notifyError(err, status === 'active' ? 'Accept Failed' : 'Reject Failed');
      });
  }, [dispatch, requestGridReload, stateRef]);

  const handleRemoveFromFolder = useCallback(() => {
    if (!folderId) return;
    const effective = selectEffectiveHashes(stateRef.current);
    const hashes = [...effective];
    if (hashes.length === 0) return;
    dispatch({ type: 'FILTER_IMAGES', predicate: (i) => !effective.has(i.hash) });
    dispatch({ type: 'CLEAR_SELECTION' });
    api.folders.removeFiles(folderId, hashes)
      .then(() => {
        registerUndoAction({
          label: `Remove ${hashes.length} from folder`,
          undo: async () => {
            await api.folders.addFiles(folderId, hashes);
            requestGridReload();
          },
          redo: async () => {
            await api.folders.removeFiles(folderId, hashes);
            requestGridReload();
          },
        });
      })
      .catch((err) => {
        notifyError(err, 'Remove from Folder Failed');
      });
  }, [folderId, stateRef, dispatch, requestGridReload]);

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
            requestGridReload();
          },
          redo: async () => {
            await api.collections.removeMembers({ id: collectionEntityId, hashes });
            requestGridReload();
          },
        });
      })
      .catch((err) => {
        notifyError(err, 'Remove from Collection Failed');
      });
  }, [collectionEntityId, stateRef, dispatch, requestGridReload]);

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
