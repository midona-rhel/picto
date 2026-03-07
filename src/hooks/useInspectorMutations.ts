import { useCallback } from 'react';
import { api } from '#desktop/api';
import { FileController } from '../shared/controllers/fileController';
import { SelectionController } from '../controllers/selectionController';
import { FolderController } from '../controllers/folderController';
import { deepClone, registerUndoAction } from '../shared/controllers/undoRedoController';
import { useCacheStore } from '../state/cacheStore';
import {
  getMetadata,
  invalidateMetadata,
  type SelectionQuerySpec,
} from '#features/grid/data';
import type { MasonryImageItem } from '#features/grid/types';
import type { InspectorFetchState } from './useInspectorFetch';

export function useInspectorMutations(
  selectedImages: MasonryImageItem[],
  selectionSummarySpec: SelectionQuerySpec | null,
  fetch: InspectorFetchState,
) {
  const {
    collectionSummary, selectedCollection, saveNotesTimer,
    setFileTags, setFileMetadata, setCollectionSummary,
    setFileFolders, setSourceUrls, setNotes,
    refreshMetadata, refreshVirtualSelectionSummary, mapCollectionTags,
  } = fetch;

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
    [selectedImages, selectionSummarySpec, selectedCollection, collectionSummary, refreshMetadata, refreshVirtualSelectionSummary, mapCollectionTags, setCollectionSummary, setFileTags],
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
    [selectedImages, selectionSummarySpec, selectedCollection, collectionSummary, refreshMetadata, refreshVirtualSelectionSummary, mapCollectionTags, setCollectionSummary, setFileTags],
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
    [selectedImages, selectionSummarySpec, selectedCollection, collectionSummary, refreshMetadata, refreshVirtualSelectionSummary, setCollectionSummary],
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
    [selectedImages, selectionSummarySpec, selectedCollection, collectionSummary, refreshMetadata, refreshVirtualSelectionSummary, setSourceUrls, setCollectionSummary],
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
    [selectedImages, selectionSummarySpec, selectedCollection, collectionSummary, saveNotesTimer, setNotes, setCollectionSummary],
  );

  const onAddToFolders = useCallback(
    async (folderIds: number[]) => {
      const hashes = selectedImages.map((img) => img.hash);
      if (selectedCollection) return;
      const folderIdsSnapshot = [...folderIds];
      const hashesSnapshot = [...hashes];
      if (folderIdsSnapshot.length === 0 || hashesSnapshot.length === 0) return;
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
    [selectedImages, selectedCollection, setFileFolders],
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
      const activeScope = useCacheStore.getState().activeGridScope;
      if (activeScope === `folder:${folderId}`) {
        useCacheStore.getState().enqueueGridRemoval(hash);
      }
    },
    [selectedImages, selectedCollection, setFileFolders],
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
    [selectedImages, selectedCollection, selectionSummarySpec, setFileMetadata, setFileTags, setSourceUrls, setNotes],
  );

  return {
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
