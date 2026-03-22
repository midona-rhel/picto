import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { filesController } from '../../../controllers/filesController';
import { foldersController } from '../../../controllers/foldersController';
import { collectionsController } from '../../../controllers/collectionsController';
import { useStateChangeStore } from '../../../runtime/stateChanges/stateChangeStore';
import {
  getOrStartSelectionSummary,
  noteMetadataChanged,
  noteSelectionSummaryChanged,
  type EntityAllMetadata,
  type ResolvedTagInfo,
  type SelectionQuerySpec,
  type SelectionSummary,
} from '#features/grid/data';
import type { MediaItem } from '#features/grid/types';
import { parseTagString } from '../../../shared/lib/tagParsing';
import type { CollectionSummary } from '../../../shared/types/api';
import type { FolderMembership } from './useInspectorData';
import { inspectorNeedsRefresh } from '../inspectorRefreshScope';

export interface InspectorFetchState {
  fileTags: ResolvedTagInfo[];
  fileMetadata: EntityAllMetadata | null;
  collectionSummary: CollectionSummary | null;
  selectionSummary: SelectionSummary | null;
  fileFolders: FolderMembership[];
  sourceUrls: string[];
  notes: string;

  // Setters (needed by inspector change actions)
  setFileTags: React.Dispatch<React.SetStateAction<ResolvedTagInfo[]>>;
  setFileMetadata: React.Dispatch<React.SetStateAction<EntityAllMetadata | null>>;
  setCollectionSummary: React.Dispatch<React.SetStateAction<CollectionSummary | null>>;
  setSelectionSummary: React.Dispatch<React.SetStateAction<SelectionSummary | null>>;
  setFileFolders: React.Dispatch<React.SetStateAction<FolderMembership[]>>;
  setSourceUrls: React.Dispatch<React.SetStateAction<string[]>>;
  setNotes: React.Dispatch<React.SetStateAction<string>>;

  // Derived
  selectedCollection: { id: number; name: string } | null;
  saveNotesTimer: React.MutableRefObject<ReturnType<typeof setTimeout> | undefined>;

  // Refresh functions
  refreshMetadata: () => void;
  refreshVirtualSelectionSummary: () => void;
  mapCollectionTags: (tags: string[]) => ResolvedTagInfo[];
}

export function useInspectorFetch(
  selectedImages: MediaItem[],
  selectionSummarySpec: SelectionQuerySpec | null,
): InspectorFetchState {
  const refreshTargetVersion = useStateChangeStore((state) => state.refreshTargetVersion);
  const [fileTags, setFileTags] = useState<ResolvedTagInfo[]>([]);
  const [fileMetadata, setFileMetadata] = useState<EntityAllMetadata | null>(null);
  const [collectionSummary, setCollectionSummary] = useState<CollectionSummary | null>(null);
  const [selectionSummary, setSelectionSummary] = useState<SelectionSummary | null>(null);
  const [fileFolders, setFileFolders] = useState<FolderMembership[]>([]);
  const [sourceUrls, setSourceUrls] = useState<string[]>([]);
  const [notes, setNotes] = useState('');

  const requestIdRef = useRef(0);
  const saveNotesTimer = useRef<ReturnType<typeof setTimeout>>();
  const selectedHashesKey = selectedImages.map((i) => i.hash).sort().join(',');
  const selectionSummaryKey = selectionSummarySpec ? JSON.stringify(selectionSummarySpec) : '';
  const firstImage = selectedImages.length === 1 ? selectedImages[0] : null;
  const selectedCollectionId = firstImage?.is_collection ? (firstImage.entity_id ?? null) : null;
  const selectedCollectionName = selectedCollectionId != null
    ? (firstImage!.name ?? `Collection ${selectedCollectionId}`)
    : null;
  const selectedCollection = useMemo(
    () => selectedCollectionId != null ? { id: selectedCollectionId, name: selectedCollectionName! } : null,
    [selectedCollectionId, selectedCollectionName],
  );

  const mapCollectionTags = useCallback((tags: string[]): ResolvedTagInfo[] => {
    return tags.map((tag) => {
      const parsed = parseTagString(tag);
      return {
        raw_tag: tag,
        display_tag: tag,
        namespace: parsed.namespace,
        subtag: parsed.subtag,
        source: 'collection',
        read_only: false,
      } satisfies ResolvedTagInfo;
    });
  }, []);

  useEffect(() => {
    if (selectedCollection) {
      setSourceUrls(collectionSummary?.source_urls ?? []);
      setNotes(collectionSummary?.notes ?? '');
      return;
    }
    setSourceUrls(fileMetadata?.entity.source_urls ?? []);
    setNotes(fileMetadata?.entity.notes?.description ?? '');
  }, [fileMetadata, collectionSummary, selectedCollection]);

  useEffect(() => {
    if (selectionSummarySpec) {
      // Virtual selection (Select All): check if scope is a folder
      const scopeSpec = selectionSummarySpec.scope;
      if (scopeSpec?.kind === 'folder' && scopeSpec.folder_id) {
        foldersController.list()
          .then((folders) => {
            const folder = folders.find((f: { folder_id: number }) => f.folder_id === scopeSpec.folder_id);
            if (folder) {
              setFileFolders([{ folder_id: folder.folder_id, folder_name: folder.name }]);
            } else {
              setFileFolders([]);
            }
          })
          .catch(() => setFileFolders([]));
      } else {
        setFileFolders([]);
      }
      return;
    }
    if (selectedCollection) {
      foldersController.getEntityFolders(selectedCollection.id)
        .then(setFileFolders)
        .catch(() => setFileFolders([]));
      return;
    }
    if (selectedImages.length === 0) {
      setFileFolders([]);
      return;
    }
    if (selectedImages.length === 1) {
      foldersController.getFileFolders(selectedImages[0].hash)
        .then(setFileFolders)
        .catch(() => setFileFolders([]));
    } else if (selectedImages.length > 1) {
      // Multi-file: compute shared folders (folders ALL selected files belong to)
      const hashes = selectedImages.slice(0, 200).map((i) => i.hash); // cap for perf
      Promise.all(hashes.map((h) => foldersController.getFileFolders(h).catch(() => [] as FolderMembership[])))
        .then((allFolders) => {
          if (allFolders.length === 0) { setFileFolders([]); return; }
          // Intersect: only keep folders present in ALL files
          const firstIds = new Set(allFolders[0].map((f) => f.folder_id));
          for (let i = 1; i < allFolders.length; i++) {
            const ids = new Set(allFolders[i].map((f) => f.folder_id));
            for (const id of firstIds) {
              if (!ids.has(id)) firstIds.delete(id);
            }
          }
          setFileFolders(allFolders[0].filter((f) => firstIds.has(f.folder_id)));
        })
        .catch(() => setFileFolders([]));
    }
  }, [selectedHashesKey, selectionSummarySpec, selectedCollection]);

  useEffect(() => {
    if (!selectionSummarySpec) {
      setSelectionSummary(null);
      return;
    }
    const requestId = ++requestIdRef.current;
    setFileMetadata(null);
    setCollectionSummary(null);
    setFileTags([]);
    setSelectionSummary(null);
    getOrStartSelectionSummary(selectionSummarySpec)
      .then((summary) => {
        if (requestIdRef.current !== requestId) return;
        setSelectionSummary(summary);
        setFileTags(
          (summary.shared_tags ?? []).map((t) => {
            const parsed = parseTagString(t.tag);
            return {
              raw_tag: t.tag,
              display_tag: t.tag,
              namespace: parsed.namespace,
              subtag: parsed.subtag,
              source: 'selection_summary',
              read_only: false,
            } satisfies ResolvedTagInfo;
          }),
        );
      })
      .catch((err) => {
        if (requestIdRef.current !== requestId) return;
        console.error('Failed to fetch selection summary:', err);
        setFileTags([]);
      });
  }, [selectionSummaryKey]);

  useEffect(() => {
    if (selectionSummarySpec) {
      setFileTags([]);
      setFileMetadata(null);
      setCollectionSummary(null);
      return;
    }
    if (selectedImages.length === 0) {
      setFileTags([]);
      setFileMetadata(null);
      setCollectionSummary(null);
      return;
    }


    const requestId = ++requestIdRef.current;

    const doFetch = async () => {
      try {
        if (selectedCollection) {
          const summary = await collectionsController.getSummary(selectedCollection.id);
          if (requestIdRef.current !== requestId) return;
          // Set new data and clear old atomically — avoids flicker frame
          setCollectionSummary(summary);
          setFileMetadata(null);
          setFileTags(mapCollectionTags(summary.tags));
          setSourceUrls(summary.source_urls ?? []);
          setNotes(summary.notes ?? '');
          return;
        }

        if (selectedImages.length === 1) {
          const metadata = await filesController.getMetadata(selectedImages[0].hash);
          if (requestIdRef.current !== requestId) return;
          // Set new data and clear old atomically — avoids flicker frame
          setFileMetadata(metadata);
          setCollectionSummary(null);
          setFileTags(metadata.tags);
        } else {
          // Multi-file: use backend SelectionSummary instead of N individual fetches
          const spec: SelectionQuerySpec = {
            mode: 'explicit_hashes',
            hashes: selectedImages.map((i) => i.hash),
            scope: { kind: 'system', system_key: 'all' },
            filters: {},
            sort: {},
            excluded_hashes: null,
            included_hashes: null,
          };
          const summary = await getOrStartSelectionSummary(spec);
          if (requestIdRef.current !== requestId) return;
          // Clear single/collection data atomically with new multi-selection data
          setCollectionSummary(null);
          setFileMetadata(null);
          setNotes('');
          setSourceUrls([]);
          setSelectionSummary(summary);
          setFileTags(
            (summary.shared_tags ?? []).map((t) => {
              const parsed = parseTagString(t.tag);
              return {
                raw_tag: t.tag,
                display_tag: t.tag,
                namespace: parsed.namespace,
                subtag: parsed.subtag,
                source: 'selection_summary',
                read_only: false,
              } satisfies ResolvedTagInfo;
            }),
          );
        }
      } catch (err) {
        if (requestIdRef.current === requestId) {
          console.error('Failed to fetch metadata:', err);
          setFileTags([]);
          setFileMetadata(null);
          setCollectionSummary(null);
        }
      }
    };

    doFetch();
  }, [selectedHashesKey, selectionSummarySpec, selectedCollection, mapCollectionTags]);

  const refreshMetadata = useCallback(() => {
    if (selectedCollection) {
      collectionsController.getSummary(selectedCollection.id)
        .then((summary) => {
          setCollectionSummary(summary);
          setFileMetadata(null);
          setFileTags(mapCollectionTags(summary.tags));
          setSourceUrls(summary.source_urls ?? []);
          setNotes(summary.notes ?? '');
        })
        .catch(() => {});
      return;
    }
    for (const img of selectedImages) noteMetadataChanged(img.hash);

    if (selectedImages.length === 1) {
      filesController.getMetadata(selectedImages[0].hash)
        .then((metadata) => {
          setFileMetadata(metadata);
          setFileTags(metadata.tags);
        })
        .catch(() => {});
    } else if (selectedImages.length > 1) {
      const spec: SelectionQuerySpec = {
        mode: 'explicit_hashes',
        hashes: selectedImages.map((i) => i.hash),
        scope: { kind: 'system', system_key: 'all' },
        filters: {},
        sort: {},
        excluded_hashes: null,
        included_hashes: null,
      };
      getOrStartSelectionSummary(spec)
        .then((summary) => {
          setSelectionSummary(summary);
          setFileTags(
            (summary.shared_tags ?? []).map((t) => {
              const parsed = parseTagString(t.tag);
              return {
                raw_tag: t.tag, display_tag: t.tag,
                namespace: parsed.namespace, subtag: parsed.subtag,
                source: 'selection_summary', read_only: false,
              } satisfies ResolvedTagInfo;
            }),
          );
        })
        .catch(() => {});
    }
  }, [selectedImages, selectedCollection, mapCollectionTags]);

  const refreshVirtualSelectionSummary = useCallback(() => {
    if (!selectionSummarySpec) return;
    const requestId = ++requestIdRef.current;
    noteSelectionSummaryChanged(selectionSummaryKey);
    setSelectionSummary(null);
    getOrStartSelectionSummary(selectionSummarySpec)
      .then((summary) => {
        if (requestIdRef.current !== requestId) return;
        setSelectionSummary(summary);
        setFileTags(
          (summary.shared_tags ?? []).map((t) => {
            const parsed = parseTagString(t.tag);
            return {
              raw_tag: t.tag,
              display_tag: t.tag,
              namespace: parsed.namespace,
              subtag: parsed.subtag,
              source: 'selection_summary',
              read_only: false,
            } satisfies ResolvedTagInfo;
          }),
        );
      })
      .catch((err) => {
        if (requestIdRef.current !== requestId) return;
        console.error('Failed to refresh selection summary:', err);
      });
  }, [selectionSummaryKey, selectionSummarySpec]);

  useEffect(() => {
    const refreshTargets = useStateChangeStore.getState().lastPlannedRefreshTargets;
    if (!inspectorNeedsRefresh({
      selectedHashes: selectedImages.map((image) => image.hash),
      hasVirtualSelection: Boolean(selectionSummarySpec),
      hasSelectedCollection: Boolean(selectedCollection),
    }, refreshTargets)) {
      return;
    }

    if (selectionSummarySpec) {
      refreshVirtualSelectionSummary();
      return;
    }

    refreshMetadata();
  }, [
    refreshTargetVersion,
    selectedImages,
    selectionSummarySpec,
    selectedCollection,
    refreshMetadata,
    refreshVirtualSelectionSummary,
  ]);

  return {
    fileTags, fileMetadata, collectionSummary, selectionSummary,
    fileFolders, sourceUrls, notes,
    setFileTags, setFileMetadata, setCollectionSummary, setSelectionSummary,
    setFileFolders, setSourceUrls, setNotes,
    selectedCollection, saveNotesTimer,
    refreshMetadata, refreshVirtualSelectionSummary, mapCollectionTags,
  };
}
