import { useEffect, useMemo, useRef, type Dispatch } from 'react';
import { useGridMetadataStore } from '../../../state/gridMetadataStore';
import { useNavigationStore } from '../../../state/navigationStore';
import type { GridRuntimeAction } from '../runtime';
import type { MasonryImageItem } from '../shared';
import type { SmartFolderPredicate } from '../../smart-folders/components/types';
import { deriveGridScopeKey } from '../scopeModel';

export function useGridRefreshLifecycle(args: {
  dispatch: Dispatch<GridRuntimeAction>;
  folderId?: number | null;
  collectionEntityId?: number | null;
  searchTags?: string[];
  excludedSearchTags?: string[];
  tagMatchMode?: 'all' | 'any' | 'exact' | null;
  smartFolderPredicate?: SmartFolderPredicate;
  filterFolderIds?: number[] | null;
  excludedFilterFolderIds?: number[] | null;
  folderMatchMode?: 'all' | 'any' | 'exact' | null;
  statusFilter?: string | null;
  requestReplace: () => Promise<void>;
  refreshTrigger?: number;
  stateRef: { current: { images: MasonryImageItem[] } };
}) {
  const {
    dispatch,
    folderId,
    collectionEntityId,
    smartFolderPredicate,
    statusFilter,
    requestReplace,
    refreshTrigger,
    stateRef,
  } = args;

  const metadataInvalidatedHashes = useGridMetadataStore((s) => s.metadataInvalidatedHashes);
  const gridRefreshSeq = useGridMetadataStore((s) => s.gridRefreshSeq);
  const activeSmartFolderId = useNavigationStore((s) => s.activeSmartFolderId);

  const activeGridScope = useMemo(() => deriveGridScopeKey({
    currentView: 'images',
    activeFolderId: folderId ?? null,
    activeCollectionId: collectionEntityId ?? null,
    activeSmartFolderId: smartFolderPredicate ? activeSmartFolderId : null,
    activeStatusFilter: statusFilter ?? null,
  }), [activeSmartFolderId, collectionEntityId, folderId, smartFolderPredicate, statusFilter]);

  useEffect(() => {
    useGridMetadataStore.getState().setActiveGridScope(activeGridScope);
  }, [activeGridScope]);

  useEffect(() => {
    if (metadataInvalidatedHashes.size === 0) return;
    const hashes = [...metadataInvalidatedHashes];
    useGridMetadataStore.getState().clearInvalidatedHashes();

    useGridMetadataStore.getState().fetchMetadataBatch(hashes).then((results) => {
      if (results.length === 0) return;
      const metaMap = new Map(results.map((r) => [r.file.hash, r.file]));
      const currentImages = stateRef.current.images;
      let changed = false;
      const next = currentImages.map((img) => {
        const meta = metaMap.get(img.hash);
        if (!meta) return img;
        if (img.name === meta.name && img.rating === meta.rating) {
          return img;
        }
        changed = true;
        return { ...img, name: meta.name, rating: meta.rating };
      });
      if (changed) dispatch({ type: 'SET_IMAGES', images: next });
    });
  }, [dispatch, metadataInvalidatedHashes, stateRef]);

  const prevRefreshTrigger = useRef(refreshTrigger);
  useEffect(() => {
    if (prevRefreshTrigger.current !== refreshTrigger) {
      prevRefreshTrigger.current = refreshTrigger;
      void requestReplace();
    }
  }, [refreshTrigger, requestReplace]);

  const prevGridRefreshSeq = useRef(gridRefreshSeq);
  useEffect(() => {
    if (prevGridRefreshSeq.current !== gridRefreshSeq) {
      prevGridRefreshSeq.current = gridRefreshSeq;
      void requestReplace();
    }
  }, [gridRefreshSeq, requestReplace]);
}
