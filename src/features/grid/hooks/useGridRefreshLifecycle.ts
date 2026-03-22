import { useEffect, useMemo, useRef, type Dispatch } from 'react';
import { useGridMetadataStore } from '../../../state/gridMetadataStore';
import { useNavigationStore } from '../../../state/navigationStore';
import type { GridRuntimeAction } from '../runtime';
import { toMasonryItem, type MasonryItem } from '../shared';
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
  stateRef: { current: { images: MasonryItem[] } };
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
  const pendingRemovals = useGridMetadataStore((s) => s.pendingRemovals);
  const pendingClearAll = useGridMetadataStore((s) => s.pendingClearAll);
  const pendingInsertions = useGridMetadataStore((s) => s.pendingInsertions);
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
    useGridMetadataStore.getState().clearChangedMetadataMarks();

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

  // Eager removals: controller queues specific hashes or a full clear to remove
  // items from the visible grid immediately (e.g. trash/delete).
  useEffect(() => {
    if (pendingRemovals.size === 0 && !pendingClearAll) return;
    const { hashes, clearAll } = useGridMetadataStore.getState().drainRemovals();
    if (clearAll) {
      dispatch({ type: 'CLEAR_DATASET' });
    } else if (hashes.size > 0) {
      dispatch({ type: 'FILTER_IMAGES', predicate: (img) => !hashes.has(img.hash) });
    }
  }, [dispatch, pendingRemovals, pendingClearAll]);

  // Eager insertions: controller queues entities that entered the current scope
  // (e.g. restore from trash while viewing system:all). Prepend to visible grid.
  useEffect(() => {
    if (pendingInsertions.length === 0) return;
    const entities = useGridMetadataStore.getState().drainInsertions();
    if (entities.length === 0) return;
    const currentHashes = new Set(stateRef.current.images.map((img) => img.hash));
    const newItems = entities
      .filter((e) => !currentHashes.has(e.hash))
      .map(toMasonryItem);
    if (newItems.length > 0) {
      dispatch({ type: 'SET_IMAGES', images: [...newItems, ...stateRef.current.images] });
    }
  }, [dispatch, pendingInsertions, stateRef]);

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
