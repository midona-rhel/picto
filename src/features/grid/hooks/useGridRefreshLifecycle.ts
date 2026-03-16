import { useEffect, useMemo, useRef, type Dispatch } from 'react';
import { useGridMetadataStore } from '../../../state/gridMetadataStore';
import type { GridRuntimeAction } from '../runtime';
import type { MasonryImageItem } from '../shared';
import type { SmartFolderPredicate } from '../../smart-folders/components/types';

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
    searchTags,
    excludedSearchTags,
    tagMatchMode,
    smartFolderPredicate,
    filterFolderIds,
    excludedFilterFolderIds,
    folderMatchMode,
    statusFilter,
    requestReplace,
    refreshTrigger,
    stateRef,
  } = args;

  const metadataInvalidatedHashes = useGridMetadataStore((s) => s.metadataInvalidatedHashes);
  const gridRefreshSeq = useGridMetadataStore((s) => s.gridRefreshSeq);

  const scopeKey = useMemo(() => JSON.stringify({
    searchTags: searchTags ?? [],
    excludedSearchTags: excludedSearchTags ?? [],
    tagMatchMode: tagMatchMode ?? null,
    smartFolderPredicate: smartFolderPredicate ? JSON.stringify(smartFolderPredicate) : null,
    folderId: folderId ?? null,
    collectionEntityId: collectionEntityId ?? null,
    filterFolderIds: filterFolderIds ?? [],
    excludedFilterFolderIds: excludedFilterFolderIds ?? [],
    folderMatchMode: folderMatchMode ?? null,
    statusFilter: statusFilter ?? null,
  }), [
    collectionEntityId,
    excludedFilterFolderIds,
    excludedSearchTags,
    filterFolderIds,
    folderId,
    folderMatchMode,
    searchTags,
    smartFolderPredicate,
    statusFilter,
    tagMatchMode,
  ]);

  useEffect(() => {
    let scope: string;
    if (collectionEntityId != null) scope = `collection:${collectionEntityId}`;
    else if (folderId != null) scope = `folder:${folderId}`;
    else if (statusFilter === 'inbox') scope = 'system:inbox';
    else if (statusFilter === 'trash') scope = 'system:trash';
    else scope = 'system:all';
    useGridMetadataStore.getState().setActiveGridScope(scope);
  }, [collectionEntityId, folderId, statusFilter, scopeKey]);

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
