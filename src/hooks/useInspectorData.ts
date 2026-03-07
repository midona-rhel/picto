import { useState, useEffect, useRef, useCallback, useMemo } from 'react';
import { api } from '#desktop/api';
import { FileController } from '../shared/controllers/fileController';
import { SelectionController } from '../controllers/selectionController';
import { FolderController } from '../controllers/folderController';
import { deepClone, registerUndoAction } from '../shared/controllers/undoRedoController';
import { useCacheStore } from '../state/cacheStore';
import {
  getMetadata,
  invalidateMetadata,
  type EntityAllMetadata,
  type ResolvedTagInfo,
  type SelectionQuerySpec,
  type SelectionSummary,
} from '#features/grid/data';
import type { MasonryImageItem } from '#features/grid/types';
import { parseTagString } from '../shared/lib/tagParsing';
import type { CollectionSummary } from '../shared/types/api';

export interface FolderMembership {
  folder_id: number;
  folder_name: string;
}

export interface InspectorData {
  fileTags: ResolvedTagInfo[];
  fileMetadata: EntityAllMetadata | null;
  collectionSummary: CollectionSummary | null;
  selectionSummary: SelectionSummary | null;
  fileFolders: FolderMembership[];
  sourceUrls: string[];
  notes: string;

  onAddTags: (tags: string[]) => Promise<void>;
  onRemoveTags: (tags: string[]) => Promise<void>;
  onUpdateRating: (rating: number) => Promise<void>;
  onUpdateSourceUrls: (urls: string[]) => Promise<void>;
  onUpdateNotes: (text: string) => void;
  onAddToFolders: (folderIds: number[]) => Promise<void>;
  onRemoveFromFolder: (folderId: number) => Promise<void>;
  onReanalyzeColors: () => Promise<void>;
}

export function useInspectorData(
  selectedImages: MasonryImageItem[],
  selectionSummarySpec: SelectionQuerySpec | null,
): InspectorData {
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

  const onAddTags = useCallback(
    async (tags: string[]) => {
      if (tags.length === 0) return;
      const tagsSnapshot = [...tags];
      if (selectionSummarySpec) {
        const specSnapshot = deepClone(selectionSummarySpec);
        await SelectionController.addTagsSelection(specSnapshot, tagsSnapshot);
        registerUndoAction({
          label: `Add ${tagsSnapshot.length} tag${tagsSnapshot.length === 1 ? '' : 's'}`,
          undo: async () => {
            await SelectionController.removeTagsSelection(specSnapshot, tagsSnapshot);
            refreshVirtualSelectionSummary();
          },
          redo: async () => {
            await SelectionController.addTagsSelection(specSnapshot, tagsSnapshot);
            refreshVirtualSelectionSummary();
          },
        });
        refreshVirtualSelectionSummary();
      } else if (selectedCollection) {
        const current = collectionSummary ?? await api.collections.getSummary(selectedCollection.id);
        const merged = Array.from(new Set([...current.tags, ...tagsSnapshot]));
        await api.collections.update({
          id: selectedCollection.id,
          name: current.name,
          description: current.description,
          tags: merged,
        });
        registerUndoAction({
          label: `Add ${tagsSnapshot.length} tag${tagsSnapshot.length === 1 ? '' : 's'}`,
          undo: async () => {
            await api.collections.update({
              id: selectedCollection.id,
              name: current.name,
              description: current.description,
              tags: current.tags,
            });
            refreshMetadata();
          },
          redo: async () => {
            await api.collections.update({
              id: selectedCollection.id,
              name: current.name,
              description: current.description,
              tags: merged,
            });
            refreshMetadata();
          },
        });
        setCollectionSummary({ ...current, tags: merged });
        setFileTags(mapCollectionTags(merged));
      } else {
        const hashes = selectedImages.map((img) => img.hash);
        if (hashes.length === 0) return;
        await api.tags.addBatch(hashes, tagsSnapshot);
        registerUndoAction({
          label: `Add ${tagsSnapshot.length} tag${tagsSnapshot.length === 1 ? '' : 's'}`,
          undo: async () => {
            await api.tags.removeBatch(hashes, tagsSnapshot);
            refreshMetadata();
          },
          redo: async () => {
            await api.tags.addBatch(hashes, tagsSnapshot);
            refreshMetadata();
          },
        });
        refreshMetadata();
      }
    },
    [selectedImages, selectionSummarySpec, selectedCollection, collectionSummary, refreshMetadata, refreshVirtualSelectionSummary, mapCollectionTags],
  );

  const onRemoveTags = useCallback(
    async (tags: string[]) => {
      if (tags.length === 0) return;
      const tagsSnapshot = [...tags];
      if (selectionSummarySpec) {
        const specSnapshot = deepClone(selectionSummarySpec);
        await SelectionController.removeTagsSelection(specSnapshot, tagsSnapshot);
        registerUndoAction({
          label: `Remove ${tagsSnapshot.length} tag${tagsSnapshot.length === 1 ? '' : 's'}`,
          undo: async () => {
            await SelectionController.addTagsSelection(specSnapshot, tagsSnapshot);
            refreshVirtualSelectionSummary();
          },
          redo: async () => {
            await SelectionController.removeTagsSelection(specSnapshot, tagsSnapshot);
            refreshVirtualSelectionSummary();
          },
        });
        refreshVirtualSelectionSummary();
      } else if (selectedCollection) {
        const current = collectionSummary ?? await api.collections.getSummary(selectedCollection.id);
        const removeSet = new Set(tagsSnapshot);
        const nextTags = current.tags.filter((t) => !removeSet.has(t));
        await api.collections.update({
          id: selectedCollection.id,
          name: current.name,
          description: current.description,
          tags: nextTags,
        });
        registerUndoAction({
          label: `Remove ${tagsSnapshot.length} tag${tagsSnapshot.length === 1 ? '' : 's'}`,
          undo: async () => {
            await api.collections.update({
              id: selectedCollection.id,
              name: current.name,
              description: current.description,
              tags: current.tags,
            });
            refreshMetadata();
          },
          redo: async () => {
            await api.collections.update({
              id: selectedCollection.id,
              name: current.name,
              description: current.description,
              tags: nextTags,
            });
            refreshMetadata();
          },
        });
        setCollectionSummary({ ...current, tags: nextTags });
        setFileTags(mapCollectionTags(nextTags));
      } else {
        const hashes = selectedImages.map((img) => img.hash);
        if (hashes.length === 0) return;
        await api.tags.removeBatch(hashes, tagsSnapshot);
        registerUndoAction({
          label: `Remove ${tagsSnapshot.length} tag${tagsSnapshot.length === 1 ? '' : 's'}`,
          undo: async () => {
            await api.tags.addBatch(hashes, tagsSnapshot);
            refreshMetadata();
          },
          redo: async () => {
            await api.tags.removeBatch(hashes, tagsSnapshot);
            refreshMetadata();
          },
        });
        for (const hash of hashes) invalidateMetadata(hash);
        setFileTags((prev) => prev.filter((t) => !tags.includes(t.raw_tag)));
      }
    },
    [selectedImages, selectionSummarySpec, selectedCollection, collectionSummary, refreshMetadata, refreshVirtualSelectionSummary, mapCollectionTags],
  );

  const onUpdateRating = useCallback(
    async (rating: number) => {
      const normalizedRating = rating || null;
      if (selectionSummarySpec) {
        await SelectionController.updateRatingSelection(selectionSummarySpec, normalizedRating);
        refreshVirtualSelectionSummary();
      } else if (selectedCollection) {
        const current = collectionSummary ?? await api.collections.getSummary(selectedCollection.id);
        const previousRating = current.rating ?? null;
        await api.collections.setRating(selectedCollection.id, normalizedRating);
        registerUndoAction({
          label: 'Update collection rating',
          undo: async () => {
            await api.collections.setRating(selectedCollection.id, previousRating);
            refreshMetadata();
          },
          redo: async () => {
            await api.collections.setRating(selectedCollection.id, normalizedRating);
            refreshMetadata();
          },
        });
        setCollectionSummary({ ...current, rating: normalizedRating });
      } else {
        const hashes = selectedImages.map((img) => img.hash);
        if (hashes.length === 0) return;
        const previousRatings = await Promise.all(
          hashes.map(async (hash) => ({
            hash,
            rating: (await getMetadata(hash)).file.rating ?? null,
          })),
        );
        await Promise.all(hashes.map((hash) => FileController.updateRating(hash, rating)));
        registerUndoAction({
          label: `Update rating (${hashes.length} image${hashes.length === 1 ? '' : 's'})`,
          undo: async () => {
            await Promise.all(
              previousRatings.map(({ hash, rating: previousRating }) =>
                api.file.updateRating(hash, previousRating),
              ),
            );
            refreshMetadata();
          },
          redo: async () => {
            await Promise.all(
              hashes.map((hash) => api.file.updateRating(hash, normalizedRating)),
            );
            refreshMetadata();
          },
        });
        refreshMetadata();
      }
    },
    [selectedImages, selectionSummarySpec, selectedCollection, collectionSummary, refreshMetadata, refreshVirtualSelectionSummary],
  );

  const onUpdateSourceUrls = useCallback(
    async (urls: string[]) => {
      setSourceUrls(urls);
      if (selectedCollection) {
        const current = collectionSummary ?? await api.collections.getSummary(selectedCollection.id);
        const previousUrls = [...(current.source_urls ?? [])];
        await api.collections.setSourceUrls(selectedCollection.id, urls);
        registerUndoAction({
          label: 'Update collection source URLs',
          undo: async () => {
            await api.collections.setSourceUrls(selectedCollection.id, previousUrls);
            refreshMetadata();
          },
          redo: async () => {
            await api.collections.setSourceUrls(selectedCollection.id, urls);
            refreshMetadata();
          },
        });
        setCollectionSummary({ ...current, source_urls: [...urls] });
        return;
      }
      if (selectionSummarySpec) {
        await SelectionController.setSourceUrlsSelection(selectionSummarySpec, urls);
        refreshVirtualSelectionSummary();
      } else {
        const hashes = selectedImages.map((img) => img.hash);
        if (hashes.length === 0) return;
        const previousUrls = await Promise.all(
          hashes.map(async (hash) => ({
            hash,
            urls: [...((await getMetadata(hash)).file.source_urls ?? [])],
          })),
        );
        await Promise.all(hashes.map((hash) => {
          invalidateMetadata(hash);
          return FileController.setSourceUrls(hash, urls);
        }));
        registerUndoAction({
          label: `Update source URLs (${hashes.length} image${hashes.length === 1 ? '' : 's'})`,
          undo: async () => {
            await Promise.all(
              previousUrls.map(({ hash, urls: prevUrls }) => api.file.setSourceUrls(hash, prevUrls)),
            );
            refreshMetadata();
          },
          redo: async () => {
            await Promise.all(
              hashes.map((hash) => api.file.setSourceUrls(hash, urls)),
            );
            refreshMetadata();
          },
        });
      }
    },
    [selectedImages, selectionSummarySpec, selectedCollection, collectionSummary, refreshMetadata, refreshVirtualSelectionSummary],
  );

  const onUpdateNotes = useCallback(
    (text: string) => {
      setNotes(text);
      if (saveNotesTimer.current) clearTimeout(saveNotesTimer.current);
      saveNotesTimer.current = setTimeout(() => {
        if (selectedCollection) {
          const current = collectionSummary;
          api.collections.update({
            id: selectedCollection.id,
            name: current?.name ?? selectedCollection.name,
            description: text,
            tags: current?.tags,
          }).then(() => {
            if (current) setCollectionSummary({ ...current, description: text });
          }).catch((e) => console.error('Failed to save collection description:', e));
          return;
        }
        const notesObj: Record<string, string> = {};
        if (text) notesObj.description = text;
        if (selectionSummarySpec) {
          SelectionController.setNotesSelection(selectionSummarySpec, notesObj)
            .catch((e) => console.error('Failed to save notes:', e));
        } else {
          if (selectedImages.length === 0) return;
          Promise.all(selectedImages.map((img) => api.file.setNotes(img.hash, notesObj)))
            .catch((e) => console.error('Failed to save notes:', e));
        }
      }, 500);
    },
    [selectedImages, selectionSummarySpec, selectedCollection, collectionSummary],
  );

  const onAddToFolders = useCallback(
    async (folderIds: number[]) => {
      const hashes = selectedImages.map((img) => img.hash);
      if (selectedCollection) return;
      const folderIdsSnapshot = [...folderIds];
      const hashesSnapshot = [...hashes];
      if (folderIdsSnapshot.length === 0 || hashesSnapshot.length === 0) return;
      // PBI-054: Batch add per folder instead of per-hash fan-out.
      await Promise.all(
        folderIdsSnapshot.map((folderId) =>
          FolderController.addFilesToFolderBatch(folderId, hashesSnapshot),
        ),
      );
      registerUndoAction({
        label: `Add to ${folderIdsSnapshot.length} folder${folderIdsSnapshot.length === 1 ? '' : 's'}`,
        undo: async () => {
          await Promise.all(
            folderIdsSnapshot.map((folderId) =>
              FolderController.removeFilesFromFolderBatch(folderId, hashesSnapshot),
            ),
          );
          if (hashesSnapshot.length === 1) {
            FolderController.getFileFolders(hashesSnapshot[0]).then(setFileFolders).catch(() => {});
          }
        },
        redo: async () => {
          await Promise.all(
            folderIdsSnapshot.map((folderId) =>
              FolderController.addFilesToFolderBatch(folderId, hashesSnapshot),
            ),
          );
          if (hashesSnapshot.length === 1) {
            FolderController.getFileFolders(hashesSnapshot[0]).then(setFileFolders).catch(() => {});
          }
        },
      });
      if (selectedImages.length === 1) {
        FolderController.getFileFolders(selectedImages[0].hash)
          .then(setFileFolders)
          .catch(() => {});
      }
    },
    [selectedImages, selectedCollection],
  );

  const onRemoveFromFolder = useCallback(
    async (folderId: number) => {
      if (selectedImages.length !== 1) return;
      if (selectedCollection) return;
      const hash = selectedImages[0].hash;
      await FolderController.removeFileFromFolder(folderId, hash);
      registerUndoAction({
        label: 'Remove from folder',
        undo: async () => {
          await FolderController.addFileToFolder(folderId, hash);
          FolderController.getFileFolders(hash).then(setFileFolders).catch(() => {});
        },
        redo: async () => {
          await FolderController.removeFileFromFolder(folderId, hash);
          FolderController.getFileFolders(hash).then(setFileFolders).catch(() => {});
        },
      });
      setFileFolders((prev) => prev.filter((f) => f.folder_id !== folderId));
      // Optimistic removal: if viewing this folder's grid, remove the image immediately
      const activeScope = useCacheStore.getState().activeGridScope;
      if (activeScope === `folder:${folderId}`) {
        useCacheStore.getState().enqueueGridRemoval(hash);
      }
    },
    [selectedImages, selectedCollection],
  );

  const onReanalyzeColors = useCallback(
    async () => {
      if (selectionSummarySpec || selectedCollection || selectedImages.length !== 1) return;
      const hash = selectedImages[0].hash;

      await FileController.reanalyzeColors(hash);
      invalidateMetadata(hash);

      const metadata = await getMetadata(hash);
      setFileMetadata(metadata);
      setFileTags(metadata.tags);
      setSourceUrls(metadata.file.source_urls ?? []);
      setNotes(metadata.file.notes?.description ?? '');
      useCacheStore.getState().invalidateHash(hash);
    },
    [selectedImages, selectedCollection, selectionSummarySpec],
  );

  return {
    fileTags,
    fileMetadata,
    collectionSummary,
    selectionSummary,
    fileFolders,
    sourceUrls,
    notes,
    onAddTags,
    onRemoveTags,
    onUpdateRating,
    onUpdateSourceUrls,
    onUpdateNotes,
    onAddToFolders,
    onRemoveFromFolder,
    onReanalyzeColors,
  };
}
