import { useEffect } from 'react';
import { listenRuntimeEvent } from '#desktop/api';
import { sortLiveImages } from '../liveSort';
import { toMasonryItem, type MasonryImageItem } from '../shared';
import type { SmartFolderPredicate } from '../../../features/smart-folders/components/types';
import type { FileImportedEvent } from '../../../shared/types/api/events';

export function useGridLiveInsertion(args: {
  dispatch: React.Dispatch<any>;
  stateRef: React.MutableRefObject<{ images: MasonryImageItem[] }>;
  sortField: string;
  sortOrder: string;
  folderId?: number | null;
  collectionEntityId?: number | null;
  smartFolderPredicate?: SmartFolderPredicate;
  searchTags?: string[];
  excludedSearchTags?: string[];
  filterFolderIds?: number[] | null;
  excludedFilterFolderIds?: number[] | null;
  ratingMin?: number | null;
  mimePrefixes?: string[] | null;
  colorHex?: string | null;
  searchText?: string;
  statusFilter?: string | null;
}) {
  const {
    dispatch,
    stateRef,
    sortField,
    sortOrder,
    folderId,
    collectionEntityId,
    smartFolderPredicate,
    searchTags,
    excludedSearchTags,
    filterFolderIds,
    excludedFilterFolderIds,
    ratingMin,
    mimePrefixes,
    colorHex,
    searchText,
    statusFilter,
  } = args;

  useEffect(() => {
    const unlisten = listenRuntimeEvent('file-imported', (event: FileImportedEvent) => {
      if (folderId != null || collectionEntityId != null || smartFolderPredicate) return;
      if (searchTags?.length || excludedSearchTags?.length || filterFolderIds?.length || excludedFilterFolderIds?.length) return;
      if (ratingMin != null || mimePrefixes?.length || colorHex || searchText) return;
      if (statusFilter === 'trash' || statusFilter === 'untagged' || statusFilter === 'uncategorized' || statusFilter === 'recently_viewed') return;
      if (statusFilter === 'inbox' && event.status !== 'inbox') return;
      if ((statusFilter == null || statusFilter === 'active') && event.status !== 'active') return;

      const nextItem = toMasonryItem({
        ...event,
        name: event.name ?? null,
        width: event.width ?? null,
        height: event.height ?? null,
        duration_ms: event.duration_ms ?? null,
        num_frames: event.num_frames ?? null,
        rating: event.rating ?? null,
        source_urls: null,
      });
      const currentImages = stateRef.current.images;
      if (currentImages.some((image) => image.hash === nextItem.hash)) return;

      dispatch({
        type: 'SET_IMAGES',
        images: sortLiveImages(
          [...currentImages, nextItem],
          sortField,
          sortOrder as 'asc' | 'desc',
        ),
      });
    });

    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [
    collectionEntityId,
    colorHex,
    dispatch,
    excludedFilterFolderIds,
    excludedSearchTags,
    filterFolderIds,
    folderId,
    mimePrefixes,
    ratingMin,
    searchTags,
    searchText,
    smartFolderPredicate,
    sortField,
    sortOrder,
    stateRef,
    statusFilter,
  ]);
}
