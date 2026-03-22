import { useCallback, useRef } from 'react';
import { entityController } from '../../../controllers/entityController';
import { tagsController } from '../../../controllers/tagsController';
import { foldersController } from '../../../controllers/foldersController';
import {
  getMetadata,
  type SelectionQuerySpec,
} from '#features/grid/data';
import type { MediaItem } from '#features/grid/types';
import type { InspectorFetchState } from './useInspectorFetch';

export function useInspectorChangeActions(
  selectedImages: MediaItem[],
  selectionSummarySpec: SelectionQuerySpec | null,
  fetch: InspectorFetchState,
) {
  const {
    selectedCollection, saveNotesTimer,
    setFileTags, setFileMetadata,
    setFileFolders, setSourceUrls, setNotes,
  } = fetch;

  const onAddTags = useCallback(
    async (tags: string[]) => {
      if (tags.length === 0) return;
      const tagsSnapshot = [...tags];
      if (selectionSummarySpec) {
        await tagsController.addToSelection(selectionSummarySpec, tagsSnapshot);
      } else {
        // Collections are media entities — resolve_hashes_batch expands
        // collection covers to include collection entity + all members.
        const hashes = selectedImages.map((img) => img.hash);
        if (hashes.length === 0) return;
        setFileTags((prev) => {
          const existing = new Set(prev.map((t) => t.raw_tag));
          const newTags = tagsSnapshot
            .filter((t) => !existing.has(t))
            .map((raw) => {
              const idx = raw.indexOf(':');
              const namespace = idx === -1 ? '' : raw.slice(0, idx);
              const subtag = idx === -1 ? raw : raw.slice(idx + 1);
              return { raw_tag: raw, display_tag: raw, namespace, subtag, source: 'local', read_only: false };
            });
          return [...prev, ...newTags];
        });
        await tagsController.addToHashes(hashes, tagsSnapshot);
      }
    },
    [selectedImages, selectionSummarySpec, setFileTags],
  );

  const onRemoveTags = useCallback(
    async (tags: string[]) => {
      if (tags.length === 0) return;
      const tagsSnapshot = [...tags];
      if (selectionSummarySpec) {
        await tagsController.removeFromSelection(selectionSummarySpec, tagsSnapshot);
      } else {
        const hashes = selectedImages.map((img) => img.hash);
        if (hashes.length === 0) return;
        setFileTags((prev) => prev.filter((t) => !tags.includes(t.raw_tag)));
        await tagsController.removeFromHashes(hashes, tagsSnapshot);
      }
    },
    [selectedImages, selectionSummarySpec, setFileTags],
  );

  const onUpdateRating = useCallback(
    async (rating: number) => {
      const normalizedRating = rating || null;
      const hashes = selectedImages.map((img) => img.hash);
      if (selectionSummarySpec) {
        const prevRatings = hashes.length > 0
          ? new Map(await Promise.all(hashes.map(async (hash) => [hash, (await getMetadata(hash)).entity.rating ?? null] as [string, number | null])))
          : undefined;
        await entityController.rateSelection(selectionSummarySpec, normalizedRating, prevRatings);
      } else {
        if (hashes.length === 0) return;
        const prevRatings = new Map(
          await Promise.all(hashes.map(async (hash) => [hash, (await getMetadata(hash)).entity.rating ?? null] as [string, number | null])),
        );
        const spec = { mode: 'explicit_hashes' as const, hashes, scope: {} as any, filters: {} as any, sort: {} as any };
        await entityController.rateSelection(spec, normalizedRating, prevRatings);
      }
    },
    [selectedImages, selectionSummarySpec],
  );

  const onUpdateSourceUrls = useCallback(
    async (urls: string[]) => {
      setSourceUrls(urls);
      if (selectionSummarySpec) {
        await entityController.setSelectionSourceUrls(selectionSummarySpec, urls);
      } else {
        const hashes = selectedImages.map((img) => img.hash);
        if (hashes.length === 0) return;
        // Controller owns undo — pass previous URLs for each hash.
        for (const hash of hashes) {
          const meta = await getMetadata(hash);
          const prev = [...(meta.entity.source_urls ?? [])];
          await entityController.setSourceUrls(hash, urls, prev);
        }
      }
    },
    [selectedImages, selectionSummarySpec, setSourceUrls],
  );

  const committedNotesRef = useRef('');
  const onUpdateNotes = useCallback(
    (text: string) => {
      setNotes(text);
      if (saveNotesTimer.current) clearTimeout(saveNotesTimer.current);
      saveNotesTimer.current = setTimeout(() => {
        if (selectedCollection) return;
        const prevNotes = committedNotesRef.current;
        const notesObj: Record<string, string> = {};
        if (text) notesObj.description = text;
        const prevObj: Record<string, string> = {};
        if (prevNotes) prevObj.description = prevNotes;
        if (selectionSummarySpec) {
          entityController.setSelectionNotes(selectionSummarySpec, notesObj)
            .then(() => { committedNotesRef.current = text; })
            .catch((e) => console.error('Failed to save notes:', e));
        } else {
          if (selectedImages.length === 0) return;
          const hashes = selectedImages.map((img) => img.hash);
          // Controller owns undo for per-hash notes.
          Promise.all(hashes.map((hash) => entityController.setNotes(hash, notesObj, prevObj)))
            .then(() => { committedNotesRef.current = text; })
            .catch((e) => console.error('Failed to save notes:', e));
        }
      }, 500);
    },
    [selectedImages, selectionSummarySpec, selectedCollection, saveNotesTimer, setNotes],
  );

  const onAddToFolders = useCallback(
    async (folderIds: number[]) => {
      const folderIdsSnapshot = [...folderIds];
      if (folderIdsSnapshot.length === 0) return;

      if (selectionSummarySpec) {
        // Virtual Select All: let the backend resolve all hashes
        await Promise.all(
          folderIdsSnapshot.map((folderId) =>
            foldersController.addFiles(folderId, [], selectionSummarySpec),
          ),
        );
        return;
      }

      // Backend folder operations expand collections transparently via resolve_hashes_batch.
      const hashes = selectedImages.map((img) => img.hash);
      const hashesSnapshot = [...hashes];
      if (hashesSnapshot.length === 0) return;
      // foldersController.addFiles owns undo registration.
      await Promise.all(
        folderIdsSnapshot.map((folderId) =>
          foldersController.addFiles(folderId, hashesSnapshot),
        ),
      );
      if (selectedImages.length === 1) {
        foldersController.getFileFolders(selectedImages[0].hash)
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
          await foldersController.removeFiles(folderId, [], selectionSummarySpec);
        } else {
          const hashes = selectedImages.map((img) => img.hash);
          if (hashes.length === 0) return;
          // foldersController.removeFiles owns undo registration.
          await foldersController.removeFiles(folderId, hashes);
        }
      } catch {
        // Revert on failure — refetch folder list
        if (selectedImages.length === 1) {
          foldersController.getFileFolders(selectedImages[0].hash).then(setFileFolders).catch(() => {});
        }
      }
    },
    [selectedImages, selectedCollection, selectionSummarySpec, setFileFolders],
  );

  const onReanalyzeColors = useCallback(
    async () => {
      if (selectionSummarySpec || selectedCollection || selectedImages.length !== 1) return;
      const hash = selectedImages[0].hash;

      // reanalyzeColors already handles cache invalidation via eagerInvalidate
      await entityController.reanalyzeColors(hash);

      const metadata = await getMetadata(hash);
      setFileMetadata(metadata);
      setFileTags(metadata.tags);
      setSourceUrls(metadata.entity.source_urls ?? []);
      setNotes(metadata.entity.notes?.description ?? '');
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
