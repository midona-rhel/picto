import { useCallback, useEffect, useRef } from 'react';
import { api, emitTo, listen } from '#desktop/api';
import { notifyError, notifySuccess } from '../../../shared/lib/notify';
import { registerUndoAction } from '../../../shared/controllers/undoRedoController';
import { logBestEffortError, runBestEffort } from '../../../shared/lib/asyncOps';
import type { MasonryImageItem } from '../shared';
import type { GridRuntimeState } from '../runtime';
import type { ViewerHostController } from '../../../features/viewer/hooks/useViewerHost';

let copiedTags: string[] | null = null;

interface UseGridItemActionsArgs {
  state: GridRuntimeState;
  stateRef: { current: GridRuntimeState };
  imagesRef: { current: MasonryImageItem[] };
  singleSelectedHash: string | null;
  viewer: ViewerHostController;
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
  viewer,
  selectedScopeCount: _selectedScopeCount,
}: UseGridItemActionsArgs): GridItemActionsResult {
  const handleOpenDetail = useCallback(() => {
    if (!singleSelectedHash) return;
    viewer.openDetail(singleSelectedHash);
  }, [singleSelectedHash, viewer]);

  const handleOpenQuickLook = useCallback(() => {
    if (!singleSelectedHash) return;
    viewer.toggleQuickLook(singleSelectedHash);
  }, [singleSelectedHash, viewer]);

  const handleOpenWithDefaultApp = useCallback(() => {
    if (!singleSelectedHash) return;
    api.file.openDefault(singleSelectedHash).catch((err) => {
      notifyError(err, 'Open Failed');
    });
  }, [singleSelectedHash]);

  const handleOpenInNewWindow = useCallback(async () => {
    if (!singleSelectedHash) return;
    const img = state.images.find((i) => i.hash === singleSelectedHash);
    api.file.openInNewWindow(singleSelectedHash, img?.width, img?.height).catch((err) => {
      notifyError(err, 'New Window Failed');
    });
  }, [singleSelectedHash, state.images]);

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
      const totalCount = stateRef.current.responseTotalCount ?? null;
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
    const totalCount = state.responseTotalCount ?? null;
    for (const label of labels) {
      emitTo(label, 'detail-images', { images: lightImages, totalCount }).catch(() => {
        logBestEffortError(`grid.emitDetailImages.refresh.${label}`, 'detail window unavailable');
        labels.delete(label);
      });
    }
  }, [state.images, state.responseTotalCount]);

  const handleRevealInFolder = useCallback(() => {
    if (!singleSelectedHash) return;
    api.file.revealInFolder(singleSelectedHash).catch((err) => {
      notifyError(err, 'Reveal Failed');
    });
  }, [singleSelectedHash]);

  const handleCopyFilePath = useCallback(async () => {
    if (!singleSelectedHash) return;
    try {
      const path = await api.file.resolvePath(singleSelectedHash);
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
      const tags = await api.tags.getForFile(hashesToCopy[0]);
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
      await api.tags.add(hashesSnapshot, tagsSnapshot);
      registerUndoAction({
        label: `Paste ${tagsSnapshot.length} tag${tagsSnapshot.length === 1 ? '' : 's'}`,
        undo: () => api.tags.remove(hashesSnapshot, tagsSnapshot),
        redo: () => api.tags.add(hashesSnapshot, tagsSnapshot),
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
