import { useCallback, useRef } from 'react';
import { api } from '#desktop/api';
import { registerUndoAction } from '../../../shared/controllers/undoRedoController';
import { useGridMetadataStore } from '../../../state/gridMetadataStore';
import {
  getMetadata,
  invalidateMetadata,
  type SelectionQuerySpec,
} from '#features/grid/data';
import type { MediaItem } from '#features/grid/types';
import type { InspectorFetchState } from './useInspectorFetch';

export function useInspectorMutations(
  selectedImages: MediaItem[],
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
        const specSnapshot = structuredClone(selectionSummarySpec);
        await api.selection.addTags(specSnapshot, tagsSnapshot);
        registerUndoAction({
          label: `Add ${tagsSnapshot.length} tag${tagsSnapshot.length === 1 ? '' : 's'}`,
          undo: async () => {
            await api.selection.removeTags(specSnapshot, tagsSnapshot);
            refreshVirtualSelectionSummary();
          },
          redo: async () => {
            await api.selection.addTags(specSnapshot, tagsSnapshot);
            refreshVirtualSelectionSummary();
          },
        });
        refreshVirtualSelectionSummary();
      } else if (selectedCollection) {
        await api.collections.addTags(selectedCollection.id, tagsSnapshot);
        registerUndoAction({
          label: `Add ${tagsSnapshot.length} tag${tagsSnapshot.length === 1 ? '' : 's'}`,
          undo: async () => {
            await api.collections.removeTags(selectedCollection.id, tagsSnapshot);
            refreshMetadata();
          },
          redo: async () => {
            await api.collections.addTags(selectedCollection.id, tagsSnapshot);
            refreshMetadata();
          },
        });
        refreshMetadata();
      } else {
        const hashes = selectedImages.map((img) => img.hash);
        if (hashes.length === 0) return;
        await api.tags.add(hashes, tagsSnapshot);
        registerUndoAction({
          label: `Add ${tagsSnapshot.length} tag${tagsSnapshot.length === 1 ? '' : 's'}`,
          undo: async () => {
            await api.tags.remove(hashes, tagsSnapshot);
            refreshMetadata();
          },
          redo: async () => {
            await api.tags.add(hashes, tagsSnapshot);
            refreshMetadata();
          },
        });
        refreshMetadata();
      }
    },
    [selectedImages, selectionSummarySpec, selectedCollection, refreshMetadata, refreshVirtualSelectionSummary, mapCollectionTags, setFileTags],
  );

  const onRemoveTags = useCallback(
    async (tags: string[]) => {
      if (tags.length === 0) return;
      const tagsSnapshot = [...tags];
      if (selectionSummarySpec) {
        const specSnapshot = structuredClone(selectionSummarySpec);
        await api.selection.removeTags(specSnapshot, tagsSnapshot);
        registerUndoAction({
          label: `Remove ${tagsSnapshot.length} tag${tagsSnapshot.length === 1 ? '' : 's'}`,
          undo: async () => {
            await api.selection.addTags(specSnapshot, tagsSnapshot);
            refreshVirtualSelectionSummary();
          },
          redo: async () => {
            await api.selection.removeTags(specSnapshot, tagsSnapshot);
            refreshVirtualSelectionSummary();
          },
        });
        refreshVirtualSelectionSummary();
      } else if (selectedCollection) {
        await api.collections.removeTags(selectedCollection.id, tagsSnapshot);
        registerUndoAction({
          label: `Remove ${tagsSnapshot.length} tag${tagsSnapshot.length === 1 ? '' : 's'}`,
          undo: async () => {
            await api.collections.addTags(selectedCollection.id, tagsSnapshot);
            refreshMetadata();
          },
          redo: async () => {
            await api.collections.removeTags(selectedCollection.id, tagsSnapshot);
            refreshMetadata();
          },
        });
        refreshMetadata();
      } else {
        const hashes = selectedImages.map((img) => img.hash);
        if (hashes.length === 0) return;
        await api.tags.remove(hashes, tagsSnapshot);
        registerUndoAction({
          label: `Remove ${tagsSnapshot.length} tag${tagsSnapshot.length === 1 ? '' : 's'}`,
          undo: async () => {
            await api.tags.add(hashes, tagsSnapshot);
            refreshMetadata();
          },
          redo: async () => {
            await api.tags.remove(hashes, tagsSnapshot);
            refreshMetadata();
          },
        });
        for (const hash of hashes) invalidateMetadata(hash);
        setFileTags((prev) => prev.filter((t) => !tags.includes(t.raw_tag)));
      }
    },
    [selectedImages, selectionSummarySpec, selectedCollection, refreshMetadata, refreshVirtualSelectionSummary, mapCollectionTags, setFileTags],
  );

  const onUpdateRating = useCallback(
    async (rating: number) => {
      const normalizedRating = rating || null;
      if (selectionSummarySpec) {
        await api.selection.updateRating(selectionSummarySpec, normalizedRating);
        registerUndoAction({
          label: 'Update rating (all selected)',
          undo: async () => {
            await api.selection.updateRating(selectionSummarySpec, null);
            refreshVirtualSelectionSummary();
          },
          redo: async () => {
            await api.selection.updateRating(selectionSummarySpec, normalizedRating);
            refreshVirtualSelectionSummary();
          },
        });
        refreshVirtualSelectionSummary();
      } else {
        const hashes = selectedImages.map((img) => img.hash);
        if (hashes.length === 0) return;
        const previousRatings = await Promise.all(
          hashes.map(async (hash) => ({
            hash,
            rating: (await getMetadata(hash)).entity.rating ?? null,
          })),
        );
        await Promise.all(hashes.map((hash) => api.files.updateRating(hash, rating)));
        registerUndoAction({
          label: `Update rating (${hashes.length} item${hashes.length === 1 ? '' : 's'})`,
          undo: async () => {
            await Promise.all(
              previousRatings.map(({ hash, rating: previousRating }) =>
                api.files.updateRating(hash, previousRating),
              ),
            );
            refreshMetadata();
          },
          redo: async () => {
            await Promise.all(
              hashes.map((hash) => api.files.updateRating(hash, normalizedRating)),
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
      if (selectionSummarySpec) {
        await api.selection.setSourceUrls(selectionSummarySpec, urls);
        refreshVirtualSelectionSummary();
      } else {
        const hashes = selectedImages.map((img) => img.hash);
        if (hashes.length === 0) return;
        const previousUrls = await Promise.all(
          hashes.map(async (hash) => ({
            hash,
            urls: [...((await getMetadata(hash)).entity.source_urls ?? [])],
          })),
        );
        await Promise.all(hashes.map((hash) => {
          invalidateMetadata(hash);
          return api.files.setSourceUrls(hash, urls);
        }));
        registerUndoAction({
          label: `Update source URLs (${hashes.length} item${hashes.length === 1 ? '' : 's'})`,
          undo: async () => {
            await Promise.all(
              previousUrls.map(({ hash, urls: prevUrls }) => api.files.setSourceUrls(hash, prevUrls)),
            );
            refreshMetadata();
          },
          redo: async () => {
            await Promise.all(
              hashes.map((hash) => api.files.setSourceUrls(hash, urls)),
            );
            refreshMetadata();
          },
        });
      }
    },
    [selectedImages, selectionSummarySpec, selectedCollection, collectionSummary, refreshMetadata, refreshVirtualSelectionSummary, setSourceUrls, setCollectionSummary],
  );

  const committedNotesRef = useRef('');
  const onUpdateNotes = useCallback(
    (text: string) => {
      setNotes(text);
      if (saveNotesTimer.current) clearTimeout(saveNotesTimer.current);
      saveNotesTimer.current = setTimeout(() => {
        if (selectedCollection) {
          return;
        }
        const prevNotes = committedNotesRef.current;
        const notesObj: Record<string, string> = {};
        if (text) notesObj.description = text;
        if (selectionSummarySpec) {
          api.selection.setNotes(selectionSummarySpec, notesObj)
            .then(() => {
              committedNotesRef.current = text;
              const prevObj: Record<string, string> = {};
              if (prevNotes) prevObj.description = prevNotes;
              registerUndoAction({
                label: 'Update notes',
                undo: async () => {
                  await api.selection.setNotes(selectionSummarySpec, prevObj);
                  setNotes(prevNotes);
                  committedNotesRef.current = prevNotes;
                },
                redo: async () => {
                  await api.selection.setNotes(selectionSummarySpec, notesObj);
                  setNotes(text);
                  committedNotesRef.current = text;
                },
              });
            })
            .catch((e) => console.error('Failed to save notes:', e));
        } else {
          if (selectedImages.length === 0) return;
          const hashes = selectedImages.map((img) => img.hash);
          Promise.all(hashes.map((hash) => api.files.setNotes(hash, notesObj)))
            .then(() => {
              committedNotesRef.current = text;
              const prevObj: Record<string, string> = {};
              if (prevNotes) prevObj.description = prevNotes;
              registerUndoAction({
                label: 'Update notes',
                undo: async () => {
                  await Promise.all(hashes.map((hash) => api.files.setNotes(hash, prevObj)));
                  setNotes(prevNotes);
                  committedNotesRef.current = prevNotes;
                },
                redo: async () => {
                  await Promise.all(hashes.map((hash) => api.files.setNotes(hash, notesObj)));
                  setNotes(text);
                  committedNotesRef.current = text;
                },
              });
            })
            .catch((e) => console.error('Failed to save notes:', e));
        }
      }, 500);
    },
    [selectedImages, selectionSummarySpec, selectedCollection, collectionSummary, saveNotesTimer, setNotes, setCollectionSummary],
  );

  const onAddToFolders = useCallback(
    async (folderIds: number[]) => {
      const folderIdsSnapshot = [...folderIds];
      if (folderIdsSnapshot.length === 0) return;

      if (selectionSummarySpec) {
        // Virtual Select All: let the backend resolve all hashes
        await Promise.all(
          folderIdsSnapshot.map((folderId) =>
            api.folders.addFiles(folderId, [], selectionSummarySpec),
          ),
        );
        return;
      }

      // For collections, use member hashes so the backend adds members
      // and sync_collection_aggregate_metadata adds the collection tile.
      let hashes: string[] = [];
      for (const img of selectedImages) {
        if (img.is_collection && img.entity_id != null) {
          const members = await api.collections.listMemberHashes(img.entity_id);
          hashes.push(...members);
        } else {
          hashes.push(img.hash);
        }
      }
      const hashesSnapshot = [...hashes];
      if (hashesSnapshot.length === 0) return;
      await Promise.all(
        folderIdsSnapshot.map((folderId) =>
          api.folders.addFiles(folderId, hashesSnapshot),
        ),
      );
      registerUndoAction({
        label: `Add to ${folderIdsSnapshot.length} folder${folderIdsSnapshot.length === 1 ? '' : 's'}`,
        undo: async () => {
          await Promise.all(
            folderIdsSnapshot.map((folderId) =>
              api.folders.removeFiles(folderId, hashesSnapshot),
            ),
          );
          if (hashesSnapshot.length === 1) {
            api.folders.getFileFolders(hashesSnapshot[0]).then(setFileFolders).catch(() => {});
          }
        },
        redo: async () => {
          await Promise.all(
            folderIdsSnapshot.map((folderId) =>
              api.folders.addFiles(folderId, hashesSnapshot),
            ),
          );
          if (hashesSnapshot.length === 1) {
            api.folders.getFileFolders(hashesSnapshot[0]).then(setFileFolders).catch(() => {});
          }
        },
      });
      if (selectedImages.length === 1) {
        api.folders.getFileFolders(selectedImages[0].hash)
          .then(setFileFolders)
          .catch(() => {});
      }
    },
    [selectedImages, selectedCollection, setFileFolders],
  );

  const onRemoveFromFolder = useCallback(
    async (folderId: number) => {
      // Optimistic: update UI immediately
      setFileFolders((prev) => prev.filter((f) => f.folder_id !== folderId));
      try {
        if (selectionSummarySpec) {
          // Virtual Select All: let the backend resolve all hashes from the selection
          await api.folders.removeFiles(folderId, [], selectionSummarySpec);
        } else {
          // For collections, use member hashes so the backend removes members
          // and sync_collection_aggregate_metadata updates the collection tile.
          let hashes: string[] = [];
          for (const img of selectedImages) {
            if (img.is_collection && img.entity_id != null) {
              const members = await api.collections.listMemberHashes(img.entity_id);
              hashes.push(...members);
            } else {
              hashes.push(img.hash);
            }
          }
          if (hashes.length === 0) return;
          await api.folders.removeFiles(folderId, hashes);
          registerUndoAction({
            label: `Remove ${hashes.length} file${hashes.length === 1 ? '' : 's'} from folder`,
            undo: async () => {
              await api.folders.addFiles(folderId, hashes);
              if (hashes.length === 1) {
                api.folders.getFileFolders(hashes[0]).then(setFileFolders).catch(() => {});
              }
            },
            redo: async () => {
              await api.folders.removeFiles(folderId, hashes);
              if (hashes.length === 1) {
                api.folders.getFileFolders(hashes[0]).then(setFileFolders).catch(() => {});
              }
            },
          });
        }
      } catch {
        // Revert on failure — refetch folder list
        if (selectedImages.length === 1) {
          api.folders.getFileFolders(selectedImages[0].hash).then(setFileFolders).catch(() => {});
        }
      }
    },
    [selectedImages, selectedCollection, selectionSummarySpec, setFileFolders],
  );

  const onReanalyzeColors = useCallback(
    async () => {
      if (selectionSummarySpec || selectedCollection || selectedImages.length !== 1) return;
      const hash = selectedImages[0].hash;

      await api.files.reanalyzeColors(hash);
      invalidateMetadata(hash);

      const metadata = await getMetadata(hash);
      setFileMetadata(metadata);
      setFileTags(metadata.tags);
      setSourceUrls(metadata.entity.source_urls ?? []);
      setNotes(metadata.entity.notes?.description ?? '');
      useGridMetadataStore.getState().invalidateHash(hash);
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
