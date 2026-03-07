import type { EntityAllMetadata, ResolvedTagInfo, SelectionQuerySpec, SelectionSummary } from '#features/grid/data';
import type { MasonryImageItem } from '#features/grid/types';
import type { CollectionSummary } from '../shared/types/api';
import { useInspectorFetch } from './useInspectorFetch';
import { useInspectorMutations } from './useInspectorMutations';

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
  const fetch = useInspectorFetch(selectedImages, selectionSummarySpec);
  const mutations = useInspectorMutations(selectedImages, selectionSummarySpec, fetch);

  return {
    fileTags: fetch.fileTags,
    fileMetadata: fetch.fileMetadata,
    collectionSummary: fetch.collectionSummary,
    selectionSummary: fetch.selectionSummary,
    fileFolders: fetch.fileFolders,
    sourceUrls: fetch.sourceUrls,
    notes: fetch.notes,
    ...mutations,
  };
}
