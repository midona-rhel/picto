import { useCallback, useEffect, useRef } from 'react';
import { api, emitTo, listen } from '#desktop/api';
import { FileController } from '../../../controllers/fileController';
import { notifyError, notifySuccess } from '../../../lib/notify';
import { registerUndoAction } from '../../../controllers/undoRedoController';
import { logBestEffortError, runBestEffort } from '../../../lib/asyncOps';
import type { MasonryImageItem } from '../shared';
import type { DetailViewControls, DetailViewState } from '../DetailView';
import type { GridRuntimeAction, GridRuntimeState } from '../runtime';

let copiedTags: string[] | null = null;

interface UseGridItemActionsArgs {
  state: GridRuntimeState;
  stateRef: { current: GridRuntimeState };
  imagesRef: { current: MasonryImageItem[] };
  singleSelectedHash: string | null;
  dispatch: React.Dispatch<GridRuntimeAction>;
  navigateToCollection: (collection: { id: number; name: string }) => void;
  onDetailViewStateChange?: (state: DetailViewState | null, controls: DetailViewControls | null) => void;
  selectedScopeCount?: number | null;
}

interface GridItemActionsResult {
  handleOpenDetail: () => void;
  handleOpenQuickLook: () => void;
  handleOpenWithDefaultApp: () => void;
  handleOpenInNewWindow: () => Promise<void>;
  handleRevealInFolder: () => void;
  handleCopyFilePath: () => Promise<void>;
  handleCopyTags: () => Promise<void>;
  handlePasteTags: () => Promise<void>;
  hasCopiedTags: boolean;
}

export function useGridItemActions({
  state,
  stateRef,
  imagesRef,
  singleSelectedHash,
  dispatch,
  selectedScopeCount,
}: UseGridItemActionsArgs): GridItemActionsResult {
  const handleOpenDetail = useCallback(() => {
    if (!singleSelectedHash) return;
    dispatch({ type: 'OPEN_DETAIL', hash: singleSelectedHash });
  }, [singleSelectedHash, dispatch]);

  const handleOpenQuickLook = useCallback(() => {
    if (state.quickLookHash) {
      dispatch({ type: 'CLOSE_QUICK_LOOK' });
    } else if (singleSelectedHash) {
      dispatch({ type: 'OPEN_QUICK_LOOK', hash: singleSelectedHash });
    }
  }, [singleSelectedHash, state.quickLookHash, dispatch]);

  const handleOpenWithDefaultApp = useCallback(() => {
    if (!singleSelectedHash) return;
    FileController.openDefault(singleSelectedHash).catch((err) => {
      notifyError(err, 'Open Failed');
    });
  }, [singleSelectedHash]);

  const handleOpenInNewWindow = useCallback(async () => {
    if (!singleSelectedHash) return;
    const img = state.images.find((i) => i.hash === singleSelectedHash);
    FileController.openInNewWindow(singleSelectedHash, img?.width, img?.height).catch((err) => {
      notifyError(err, 'New Window Failed');
    });
  }, [singleSelectedHash, state.images]);

  const selectedScopeCountRef = useRef(selectedScopeCount);
  selectedScopeCountRef.current = selectedScopeCount;

  const detailWindowLabelsRef = useRef<Set<string>>(new Set());
  useEffect(() => {
    const unlisten = listen<{ hash: string }>('detail-window-ready', (event) => {
      const { hash: reqHash } = event.payload;
      const label = `detail-${reqHash.slice(0, 12)}`;
      detailWindowLabelsRef.current.add(label);
      const lightImages = imagesRef.current.map((i) => ({
        hash: i.hash,
        name: i.name,
        mime: i.mime,
        width: i.width,
        height: i.height,
      }));
      const totalCount = stateRef.current.responseTotalCount ?? selectedScopeCountRef.current ?? null;
      runBestEffort(`grid.emitDetailImages.${label}`, emitTo(label, 'detail-images', { images: lightImages, totalCount }));
    });
    return () => {
      runBestEffort('grid.unlistenDetailWindowReady', unlisten.then((fn) => fn()));
    };
  }, [imagesRef, stateRef]);

  useEffect(() => {
    const labels = detailWindowLabelsRef.current;
    if (labels.size === 0) return;
    const lightImages = state.images.map((i) => ({
      hash: i.hash,
      name: i.name,
      mime: i.mime,
      width: i.width,
      height: i.height,
    }));
    const totalCount = state.responseTotalCount ?? selectedScopeCountRef.current ?? null;
    for (const label of labels) {
      emitTo(label, 'detail-images', { images: lightImages, totalCount }).catch(() => {
        logBestEffortError(`grid.emitDetailImages.refresh.${label}`, 'detail window unavailable');
        labels.delete(label);
      });
    }
  }, [state.images, state.responseTotalCount]);

  const handleRevealInFolder = useCallback(() => {
    if (!singleSelectedHash) return;
    FileController.revealInFolder(singleSelectedHash).catch((err) => {
      notifyError(err, 'Reveal Failed');
    });
  }, [singleSelectedHash]);

  const handleCopyFilePath = useCallback(async () => {
    if (!singleSelectedHash) return;
    try {
      const path = await FileController.resolveFilePath(singleSelectedHash);
      await navigator.clipboard.writeText(path);
      notifySuccess('File path copied to clipboard', 'Copied');
    } catch (err) {
      notifyError(err, 'Copy Failed');
    }
  }, [singleSelectedHash]);

  const handleCopyTags = useCallback(async () => {
    const { virtualAllSelection, selectedHashes, images } = stateRef.current;
    const hashesToCopy = virtualAllSelection
      ? images.filter((i) => !virtualAllSelection.excludedHashes.has(i.hash)).map((i) => i.hash)
      : [...selectedHashes];
    if (hashesToCopy.length === 0) return;
    try {
      const tags = await FileController.getFileTags(hashesToCopy[0]);
      copiedTags = tags.map((t) => t.display);
      notifySuccess(`${copiedTags.length} tag(s) copied`, 'Tags Copied');
    } catch (err) {
      notifyError(err, 'Copy Tags Failed');
    }
  }, [stateRef]);

  const handlePasteTags = useCallback(async () => {
    if (!copiedTags || copiedTags.length === 0) return;
    const { virtualAllSelection, selectedHashes, images } = stateRef.current;
    const hashesToPaste = virtualAllSelection
      ? images.filter((i) => !virtualAllSelection.excludedHashes.has(i.hash)).map((i) => i.hash)
      : [...selectedHashes];
    if (hashesToPaste.length === 0) return;
    try {
      const tagsSnapshot = [...copiedTags];
      const hashesSnapshot = [...hashesToPaste];
      await api.tags.addBatch(hashesSnapshot, tagsSnapshot);
      registerUndoAction({
        label: `Paste ${tagsSnapshot.length} tag${tagsSnapshot.length === 1 ? '' : 's'}`,
        undo: () => api.tags.removeBatch(hashesSnapshot, tagsSnapshot),
        redo: () => api.tags.addBatch(hashesSnapshot, tagsSnapshot),
      });
      notifySuccess(
        `Applied ${copiedTags.length} tag(s) to ${hashesToPaste.length} file(s)`,
        'Tags Pasted',
      );
    } catch (err) {
      notifyError(err, 'Paste Tags Failed');
    }
  }, [stateRef]);

  return {
    handleOpenDetail,
    handleOpenQuickLook,
    handleOpenWithDefaultApp,
    handleOpenInNewWindow,
    handleRevealInFolder,
    handleCopyFilePath,
    handleCopyTags,
    handlePasteTags,
    hasCopiedTags: !!copiedTags && copiedTags.length > 0,
  };
}
