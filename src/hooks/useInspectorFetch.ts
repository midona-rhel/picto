import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { api } from '#desktop/api';
import { SelectionController } from '../controllers/selectionController';
import { FolderController } from '../controllers/folderController';
import {
  invalidateMetadata,
  type EntityAllMetadata,
  type ResolvedTagInfo,
  type SelectionQuerySpec,
  type SelectionSummary,
} from '#features/grid/data';
import type { MasonryImageItem } from '#features/grid/types';
import { parseTagString } from '../shared/lib/tagParsing';
import type { CollectionSummary } from '../shared/types/api';
import type { FolderMembership } from './useInspectorData';

export interface InspectorFetchState {
  fileTags: ResolvedTagInfo[];
  fileMetadata: EntityAllMetadata | null;
  collectionSummary: CollectionSummary | null;
  selectionSummary: SelectionSummary | null;
  fileFolders: FolderMembership[];
  sourceUrls: string[];
  notes: string;

  // Setters (needed by mutations hook)
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
  selectedImages: MasonryImageItem[],
  selectionSummarySpec: SelectionQuerySpec | null,
): InspectorFetchState {
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
      setNotes(collectionSummary?.description ?? '');
      return;
    }
    setSourceUrls(fileMetadata?.file.source_urls ?? []);
    setNotes(fileMetadata?.file.notes?.description ?? '');
  }, [fileMetadata, collectionSummary, selectedCollection]);

  useEffect(() => {
    if (selectionSummarySpec) {
      setFileFolders([]);
      return;
    }
    if (selectedCollection) {
      FolderController.getEntityFolders(selectedCollection.id)
        .then(setFileFolders)
        .catch(() => setFileFolders([]));
      return;
    }
    if (selectedImages.length === 0) {
      setFileFolders([]);
      return;
    }
    if (selectedImages.length === 1) {
      FolderController.getFileFolders(selectedImages[0].hash)
        .then(setFileFolders)
        .catch(() => setFileFolders([]));
    } else {
      Promise.all(selectedImages.map((img) => FolderController.getFileFolders(img.hash)))
        .then((allFolders) => {
          if (allFolders.length === 0) { setFileFolders([]); return; }
          const first = allFolders[0];
          const shared = first.filter((f) =>
            allFolders.every((folders) => folders.some((ff) => ff.folder_id === f.folder_id)),
          );
          setFileFolders(shared);
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
    SelectionController.getOrStartSummary(selectionSummarySpec)
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

    const selectionTime = performance.now();
    const hashPreview = selectedImages.map((i) => i.hash.slice(0, 8)).join(', ');
    if (import.meta.env.DEV) console.log(`[props-perf] selection changed → [${hashPreview}]`);

    const requestId = ++requestIdRef.current;

    const doFetch = async () => {
      try {
        if (selectedCollection) {
          const summary = await api.collections.getSummary(selectedCollection.id);
          if (requestIdRef.current !== requestId) return;
          setCollectionSummary(summary);
          setFileMetadata(null);
          setFileTags(mapCollectionTags(summary.tags));
          setSourceUrls(summary.source_urls ?? []);
          setNotes(summary.description ?? '');
          return;
        }

        if (selectedImages.length === 1) {
          setCollectionSummary(null);
          const metadata = await api.file.getAllMetadata(selectedImages[0].hash);
          if (requestIdRef.current !== requestId) return;
          if (import.meta.env.DEV) console.log(
            `[props-perf] metadata applied for [${hashPreview}] — ${(performance.now() - selectionTime).toFixed(1)}ms total, ${metadata.tags.length} tags`,
          );
          setFileMetadata(metadata);
          setFileTags(metadata.tags);
        } else {
          setCollectionSummary(null);
          const allMetadata = await Promise.all(selectedImages.map((img) => api.file.getAllMetadata(img.hash)));
          if (requestIdRef.current !== requestId) return;
          if (allMetadata.length === 0) {
            setFileTags([]);
            setFileMetadata(null);
            setNotes('');
            setSourceUrls([]);
            return;
          }
          const first = allMetadata[0].tags;
          const shared = first.filter((tag) =>
            allMetadata.every((m) => m.tags.some((t) => t.raw_tag === tag.raw_tag)),
          );
          const firstNotes = allMetadata[0].file.notes?.description ?? '';
          const allNotesMatch = allMetadata.every((m) => (m.file.notes?.description ?? '') === firstNotes);
          const firstUrls = allMetadata[0].file.source_urls ?? [];
          const firstUrlsKey = JSON.stringify(firstUrls);
          const allUrlsMatch = allMetadata.every((m) => JSON.stringify(m.file.source_urls ?? []) === firstUrlsKey);
          if (import.meta.env.DEV) console.log(
            `[props-perf] metadata applied for [${hashPreview}] — ${(performance.now() - selectionTime).toFixed(1)}ms total, ${shared.length} shared tags`,
          );
          setFileMetadata(null);
          setFileTags(shared);
          setNotes(allNotesMatch ? firstNotes : '');
          setSourceUrls(allUrlsMatch ? firstUrls : []);
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
      api.collections.getSummary(selectedCollection.id)
        .then((summary) => {
          setCollectionSummary(summary);
          setFileMetadata(null);
          setFileTags(mapCollectionTags(summary.tags));
          setSourceUrls(summary.source_urls ?? []);
          setNotes(summary.description ?? '');
        })
        .catch(() => {});
      return;
    }
    for (const img of selectedImages) invalidateMetadata(img.hash);

    if (selectedImages.length === 1) {
      api.file.getAllMetadata(selectedImages[0].hash)
        .then((metadata) => {
          setFileMetadata(metadata);
          setFileTags(metadata.tags);
        })
        .catch(() => {});
    } else if (selectedImages.length > 1) {
      Promise.all(selectedImages.map((img) => api.file.getAllMetadata(img.hash)))
        .then((allMetadata) => {
          if (allMetadata.length === 0) {
            setFileTags([]);
            return;
          }
          const first = allMetadata[0].tags;
          const shared = first.filter((tag) =>
            allMetadata.every((m) => m.tags.some((t) => t.raw_tag === tag.raw_tag)),
          );
          setFileTags(shared);
        })
        .catch(() => {});
    }
  }, [selectedImages, selectedCollection, mapCollectionTags]);

  const refreshVirtualSelectionSummary = useCallback(() => {
    if (!selectionSummarySpec) return;
    const requestId = ++requestIdRef.current;
    SelectionController.invalidateSummary(selectionSummaryKey);
    setSelectionSummary(null);
    SelectionController.getOrStartSummary(selectionSummarySpec)
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

  return {
    fileTags, fileMetadata, collectionSummary, selectionSummary,
    fileFolders, sourceUrls, notes,
    setFileTags, setFileMetadata, setCollectionSummary, setSelectionSummary,
    setFileFolders, setSourceUrls, setNotes,
    selectedCollection, saveNotesTimer,
    refreshMetadata, refreshVirtualSelectionSummary, mapCollectionTags,
  };
}
