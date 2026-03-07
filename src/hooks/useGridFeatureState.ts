/**
 * useGridFeatureState — search tags/text, filter bar, flow modal, folder sort
 * actions, smart-folder refresh, color debounce, and active scope count.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { api } from '#desktop/api';
import { useFilterStore, mimeFilterToPrefixes, type FilterLogicMode } from '../state/filterStore';
import { FolderController } from '../controllers/folderController';
import { applyGridMutationEffects } from '../domain/actions/mutationEffects';
import type { SmartFolder } from '#features/smart-folders/types';
import type { TagFilterLogicMode } from '#features/tags/types';
import type { MasonryImageItem } from '#features/grid/types';
import type { FlowResultEntry } from '#features/subscriptions/components';

export interface GridFeatureParams {
  currentView: string;
  isDetailMode: boolean;
  activeFolder: { folder_id: number } | null;
  activeSmartFolder: SmartFolder | null;
  setActiveSmartFolder: (sf: SmartFolder) => void;
  filterTags: string[] | null;
  allImagesCount: number | null;
  activeStatusFilter: string | null;
  inboxCount: number | null;
  uncategorizedCount: number | null;
  trashCount: number | null;
  smartFolderCounts: Record<string, number>;
  folderNodes: Array<{ id: string; count?: number | null }>;
  selectedImages: MasonryImageItem[];
}

export interface GridFeatureState {
  searchTags: string[];
  setSearchTags: (tags: string[]) => void;
  searchText: string;
  setSearchText: (text: string) => void;
  flowLastResults: Record<number, FlowResultEntry>;
  setFlowLastResults: React.Dispatch<React.SetStateAction<Record<number, FlowResultEntry>>>;
  createFlowModalOpen: boolean;
  setCreateFlowModalOpen: (v: boolean) => void;
  effectiveSearchTags: string[];
  smartFolderRefresh: number;
  handleSmartFolderUpdated: () => Promise<void>;
  handleSortFolderAction: (sortBy: string, direction: string) => void;
  handleReverseFolderAction: () => void;
  handleReverseSelectedAction: () => void;
  // Filter bar
  filterBarOpen: boolean;
  ratingFilter: number | null;
  mimeFilter: ReturnType<typeof useFilterStore.getState>['mimeFilter'];
  mimePrefixes: string[] | null;
  showFilterBar: boolean;
  filterFolderIds: number[] | null;
  excludedFilterFolderIds: number[] | null;
  folderMatchMode: 'all' | 'any' | 'exact';
  filterSearchText: string;
  excludedSearchTags: string[];
  setExcludedSearchTags: (tags: string[]) => void;
  tagLogicMode: TagFilterLogicMode;
  setTagLogicMode: (mode: TagFilterLogicMode) => void;
  tagMatchMode: 'all' | 'any' | 'exact';
  debouncedColorHex: string | null;
  debouncedColorAccuracy: number;
  activeGridScopeCount: number | null;
}

export function useGridFeatureState({
  currentView,
  isDetailMode,
  activeFolder,
  activeSmartFolder,
  setActiveSmartFolder,
  filterTags,
  allImagesCount,
  activeStatusFilter,
  inboxCount,
  uncategorizedCount,
  trashCount,
  smartFolderCounts,
  folderNodes,
  selectedImages,
}: GridFeatureParams): GridFeatureState {
  // --- Search ---
  const [searchTags, setSearchTags] = useState<string[]>([]);
  const [excludedSearchTags, setExcludedSearchTags] = useState<string[]>([]);
  const [tagLogicMode, setTagLogicMode] = useState<TagFilterLogicMode>('OR');
  const [searchText, setSearchText] = useState('');
  const [flowLastResults, setFlowLastResults] = useState<Record<number, FlowResultEntry>>({});
  const [createFlowModalOpen, setCreateFlowModalOpen] = useState(false);

  const effectiveSearchTags = useMemo(() => {
    if (!filterTags || filterTags.length === 0) return searchTags;
    if (searchTags.length === 0) return filterTags;
    return [...filterTags, ...searchTags];
  }, [filterTags, searchTags]);

  // --- Smart folder refresh ---
  const [smartFolderRefresh, setSmartFolderRefresh] = useState(0);

  const handleSmartFolderUpdated = useCallback(async () => {
    if (!activeSmartFolder?.id) return;
    try {
      const folders = await api.smartFolders.list();
      const updated = folders.find((f) => f.id === activeSmartFolder.id);
      if (updated) {
        setActiveSmartFolder(updated);
        setSmartFolderRefresh((c) => c + 1);
      }
    } catch (e) {
      console.error('Failed to refresh active smart folder:', e);
    }
  }, [activeSmartFolder?.id, setActiveSmartFolder]);

  // --- Folder sort actions ---
  const handleSortFolderAction = useCallback((sortBy: string, direction: string) => {
    const fid = activeFolder?.folder_id;
    if (!fid) return;
    FolderController.sortFolderItems(fid, sortBy, direction)
      .then(() => applyGridMutationEffects())
      .catch((e) => console.error('Failed to sort folder items:', e));
  }, [activeFolder?.folder_id]);

  const handleReverseFolderAction = useCallback(() => {
    const fid = activeFolder?.folder_id;
    if (!fid) return;
    FolderController.reverseFolderItems(fid)
      .then(() => applyGridMutationEffects())
      .catch((e) => console.error('Failed to reverse folder items:', e));
  }, [activeFolder?.folder_id]);

  const handleReverseSelectedAction = useCallback(() => {
    const fid = activeFolder?.folder_id;
    if (!fid) return;
    const hashes = selectedImages.map((img) => img.hash);
    if (hashes.length === 0) return;
    FolderController.reverseFolderItems(fid, hashes)
      .then(() => applyGridMutationEffects())
      .catch((e) => console.error('Failed to reverse selected folder items:', e));
  }, [activeFolder?.folder_id, selectedImages]);

  // --- Filter bar ---
  const filterBarOpen = useFilterStore((s) => s.filterBarOpen);
  const ratingFilter = useFilterStore((s) => s.ratingFilter);
  const mimeFilter = useFilterStore((s) => s.mimeFilter);
  const colorFilter = useFilterStore((s) => s.colorFilter);
  const colorAccuracy = useFilterStore((s) => s.colorAccuracy);
  const folderFilter = useFilterStore((s) => s.folderFilter);
  const folderFilterMode = useFilterStore((s) => s.folderFilterMode);
  const filterSearchText = useFilterStore((s) => s.searchText);
  const filterFolderIds = folderFilter.includes.size > 0 ? [...folderFilter.includes.keys()] : null;
  const excludedFilterFolderIds = folderFilter.excludes.size > 0 ? [...folderFilter.excludes.keys()] : null;
  const isImagesView = currentView === 'images';
  const showFilterBar = filterBarOpen && isImagesView && !isDetailMode;
  const mimePrefixes = mimeFilterToPrefixes(mimeFilter);

  const toBackendMatchMode = useCallback((mode: FilterLogicMode | TagFilterLogicMode): 'all' | 'any' | 'exact' => {
    if (mode === 'AND') return 'all';
    if (mode === 'EQUAL') return 'exact';
    return 'any';
  }, []);
  const folderMatchMode = toBackendMatchMode(folderFilterMode);
  const tagMatchMode = useMemo(() => {
    // Navigation-pinned tags are always treated as required.
    if (filterTags && filterTags.length > 0) return 'all';
    return toBackendMatchMode(tagLogicMode);
  }, [filterTags, tagLogicMode, toBackendMatchMode]);

  // Debounce color filter values
  const [debouncedColorHex, setDebouncedColorHex] = useState<string | null>(colorFilter);
  const [debouncedColorAccuracy, setDebouncedColorAccuracy] = useState(colorAccuracy);
  const colorDebounceRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    if (colorDebounceRef.current) clearTimeout(colorDebounceRef.current);
    colorDebounceRef.current = setTimeout(() => {
      setDebouncedColorHex(colorFilter);
      setDebouncedColorAccuracy(colorAccuracy);
    }, 400);
    return () => { if (colorDebounceRef.current) clearTimeout(colorDebounceRef.current); };
  }, [colorFilter, colorAccuracy]);

  useEffect(() => {
    document.documentElement.style.setProperty(
      '--filter-bar-height',
      showFilterBar ? '40px' : '0px',
    );
    if (showFilterBar) {
      document.documentElement.dataset.filterBarOpen = '1';
    } else {
      delete document.documentElement.dataset.filterBarOpen;
    }
  }, [showFilterBar]);

  // --- Scope count ---
  const activeFolderCount = activeFolder
    ? (folderNodes.find((n) => n.id === `folder:${activeFolder.folder_id}`)?.count ?? null)
    : null;
  const statusFilterCount =
    activeStatusFilter === 'inbox' ? inboxCount
    : activeStatusFilter === 'uncategorized' ? uncategorizedCount
    : activeStatusFilter === 'trash' ? trashCount
    : null;
  const activeGridScopeCount = activeFolder
    ? activeFolderCount
    : activeSmartFolder?.id
      ? (smartFolderCounts[activeSmartFolder.id] ?? null)
      : statusFilterCount ?? allImagesCount;

  return {
    searchTags, setSearchTags,
    searchText, setSearchText,
    flowLastResults, setFlowLastResults,
    createFlowModalOpen, setCreateFlowModalOpen,
    effectiveSearchTags,
    smartFolderRefresh, handleSmartFolderUpdated,
    handleSortFolderAction, handleReverseFolderAction, handleReverseSelectedAction,
    filterBarOpen, ratingFilter, mimeFilter, mimePrefixes, showFilterBar,
    filterFolderIds, excludedFilterFolderIds, folderMatchMode, filterSearchText,
    excludedSearchTags, setExcludedSearchTags, tagLogicMode, setTagLogicMode, tagMatchMode,
    debouncedColorHex, debouncedColorAccuracy,
    activeGridScopeCount,
  };
}
