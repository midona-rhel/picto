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

/* ------------------------------------------------------------------ */
/*  Shared helpers                                                     */
/* ------------------------------------------------------------------ */

const plural = (n: number) => `${n.toLocaleString()} image${n === 1 ? '' : 's'}`;

/** Optimistically remove matching images and clear selection. */
function optimisticRemove(
  state: GridRuntimeState,
  dispatch: React.Dispatch<GridRuntimeAction>,
) {
  const { virtualAllSelection } = state;
  if (virtualAllSelection) {
    dispatch({ type: 'FILTER_IMAGES', predicate: (i) => virtualAllSelection.excludedHashes.has(i.hash) });
  } else {
    const sel = state.selectedHashes;
    dispatch({ type: 'FILTER_IMAGES', predicate: (i) => !sel.has(i.hash) });
  }
  dispatch({ type: 'CLEAR_SELECTION' });
}

/** Run a status-change mutation on the current selection, with undo/redo and notification. */
function statusMutation(
  state: GridRuntimeState,
  dispatch: React.Dispatch<GridRuntimeAction>,
  targetStatus: string,
  undoStatus: string,
  label: (n: number) => string,
  successTitle: string,
  errorTitle: string,
  requestGridReload: () => void,
) {
  const { virtualAllSelection, selectedHashes } = state;

  optimisticRemove(state, dispatch);

  const spec = virtualAllSelection
    ? selectVirtualSpec(state)!
    : buildExplicitSelectionSpec([...selectedHashes]);

  if (!virtualAllSelection && selectedHashes.size === 0) return;

  const specSnapshot = structuredClone(spec);

  api.file.setStatusSelection(spec, targetStatus)
    .then((count) => {
      const n = Number(count ?? selectedHashes.size);
      registerUndoAction({
        label: label(n),
        undo: async () => { await api.file.setStatusSelection(specSnapshot, undoStatus); requestGridReload(); },
        redo: async () => { await api.file.setStatusSelection(specSnapshot, targetStatus); requestGridReload(); },
      });
      notifyInfo(`${label(n)}`, successTitle);
    })
    .catch((err) => notifyError(err, errorTitle));
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
    const state = stateRef.current;
    const { virtualAllSelection, selectedHashes, images } = state;
    const inTrash = statusFilter === 'trash';

    // Permanent delete in trash: no undo
    if (inTrash) {
      optimisticRemove(state, dispatch);
      if (virtualAllSelection) {
        const spec = selectVirtualSpec(state)!;
        api.file.deleteSelection(spec)
          .then((count) => notifyInfo(`${plural(Number(count ?? 0))} permanently deleted`, 'Deleted'))
          .catch((err) => notifyError(err, 'Delete Failed'));
      } else if (selectedHashes.size > 0) {
        api.file.deleteMany([...selectedHashes]).catch((err) => notifyError(err, 'Delete Failed'));
      }
      return;
    }

    // Move to trash with per-item status restore for explicit selection
    if (!virtualAllSelection) {
      if (selectedHashes.size === 0) return;
      const hashes = [...selectedHashes];
      const previousStatuses = hashes.map((hash) => ({
        hash,
        status: images.find((img) => img.hash === hash)?.status ?? (statusFilter ?? 'active'),
      }));
      const explicitSpec = buildExplicitSelectionSpec(hashes);
      optimisticRemove(state, dispatch);
      api.file.setStatusSelection(explicitSpec, 'trash')
        .then(() => {
          registerUndoAction({
            label: `Move ${plural(hashes.length)} to trash`,
            undo: async () => { await restoreStatusesByHash(previousStatuses); requestGridReload(); },
            redo: async () => { await api.file.setStatusSelection(explicitSpec, 'trash'); requestGridReload(); },
          });
        })
        .catch((err) => notifyError(err, 'Delete Failed'));
      return;
    }

    // Virtual all-selection: move to trash
    const undoStatus = statusFilter ?? 'active';
    statusMutation(
      state, dispatch, 'trash', undoStatus,
      (n) => `Move ${plural(n)} to trash`,
      'Moved to Trash', 'Delete Failed', requestGridReload,
    );
  }, [stateRef, statusFilter, dispatch, restoreStatusesByHash, requestGridReload]);

  const handleRateSelected = useCallback((rating: number) => {
    const { virtualAllSelection, selectedHashes } = stateRef.current;
    const normalizedRating = rating || null;
    if (virtualAllSelection) {
      const spec = selectVirtualSpec(stateRef.current)!;
      api.selection.updateRating(spec, normalizedRating).catch((err) => notifyError(err, 'Rating Failed'));
      return;
    }
    const hashes = [...selectedHashes];
    if (hashes.length === 0) return;
    Promise.all(hashes.map((hash) => api.file.updateRating(hash, rating))).catch((err) =>
      notifyError(err, 'Rating Failed'),
    );
  }, [stateRef]);

  const handleRestoreSelected = useCallback(() => {
    statusMutation(
      stateRef.current, dispatch, 'active', 'trash',
      (n) => `Restore ${plural(n)}`,
      'Restored', 'Restore Failed', requestGridReload,
    );
  }, [stateRef, dispatch, requestGridReload]);

  const handleInboxAction = useCallback((hash: string, status: 'active' | 'trash') => {
    api.file.setStatus(hash, status)
      .then(() => {
        registerUndoAction({
          label: status === 'active' ? 'Accept inbox image' : 'Reject inbox image',
          undo: async () => { await api.file.setStatus(hash, 'inbox'); requestGridReload(); },
          redo: async () => { await api.file.setStatus(hash, status); requestGridReload(); },
        });
      })
      .catch((err) => notifyError(err, status === 'active' ? 'Accept Failed' : 'Reject Failed'));
    dispatch({ type: 'FILTER_IMAGES', predicate: (i) => i.hash !== hash });
    dispatch({ type: 'REMOVE_HASHES', hashes: new Set([hash]) });
  }, [dispatch, requestGridReload]);

  const handleInboxSelectionAction = useCallback((status: 'active' | 'trash') => {
    const verb = status === 'active' ? 'Accept' : 'Reject';
    const past = status === 'active' ? 'Accepted' : 'Rejected';
    statusMutation(
      stateRef.current, dispatch, status, 'inbox',
      (n) => `${verb} ${plural(n)}`,
      past, `${verb} Failed`, requestGridReload,
    );
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
          undo: async () => { await api.folders.addFiles(folderId, hashes); requestGridReload(); },
          redo: async () => { await api.folders.removeFiles(folderId, hashes); requestGridReload(); },
        });
      })
      .catch((err) => notifyError(err, 'Remove from Folder Failed'));
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
          undo: async () => { await api.collections.addMembers({ id: collectionEntityId, hashes }); requestGridReload(); },
          redo: async () => { await api.collections.removeMembers({ id: collectionEntityId, hashes }); requestGridReload(); },
        });
      })
      .catch((err) => notifyError(err, 'Remove from Collection Failed'));
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
