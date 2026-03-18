/**
 * Navigation store — single source of truth for app navigation state.
 *
 * Replaces the 10+ useState calls in App.tsx for view type, active folder,
 * active smart folder, history stack, etc.
 */

import { create } from 'zustand';
import { useDomainStore } from './domainStore';
import type { SmartFolder } from '#features/smart-folders/types';

export type ViewType = 'images' | 'collections' | 'subscriptions' | 'duplicates' | 'tags';

export const VIEW_LABELS: Record<ViewType, string> = {
  images: 'All Active',
  collections: 'Albums',
  subscriptions: 'Subscriptions',
  duplicates: 'Duplicates',
  tags: 'Tags',
};

interface HistoryEntry {
  view: ViewType;
  smartFolderId: string | null;
  folderId: number | null;
  collectionId: number | null;
  statusFilter: string | null;
  filterTags: string[] | null;
  scrollTop: number;
  loadedItemCount: number;
  randomSeed: number | null;
}

interface NavigationState {
  // Current state
  currentView: ViewType;
  activeSmartFolderId: string | null;
  activeFolderId: number | null;
  activeCollectionId: number | null;
  activeStatusFilter: string | null;
  filterTags: string[] | null;

  // History
  history: HistoryEntry[];
  historyIndex: number;
  canGoBack: boolean;
  canGoForward: boolean;

  // Scroll restore for back/forward navigation
  pendingScrollRestore: number | null;
  pendingLoadedItemCount: number;
  pendingRandomSeed: number | null;

  // Actions
  navigateTo: (view: ViewType, smartFolderId?: string | null, folderId?: number | null, statusFilter?: string | null) => void;
  goBack: () => void;
  goForward: () => void;
  /** Save current scroll position to the current history entry */
  saveScrollTop: (scrollTop: number, expectedHistoryIndex?: number) => void;
  /** Save loaded item count to the current history entry */
  saveLoadedItemCount: (count: number) => void;
  /** Save random seed to the current history entry */
  saveRandomSeed: (seed: number | null) => void;
  /** Consume the pending scroll restore value (returns it and clears it) */
  consumeScrollRestore: () => number | null;
  setActiveFolderId: (folderId: number | null) => void;
  /** Navigate to a folder (sets view to 'images', clears smart folder) */
  navigateToFolder: (folder: number | { folder_id: number; name?: string }) => void;
  /** Navigate to a smart folder (sets view to 'images', clears folder) */
  navigateToSmartFolder: (folder: SmartFolder) => void;
  /** Navigate to a collection drill-down session (images view scoped to collection members). */
  navigateToCollection: (collection: number | { id: number; name?: string }) => void;
  /** Navigate to images view filtered by specific tags */
  navigateToFilterTags: (tags: string[]) => void;
}

export function deriveNavigationTitle(state: { activeFolderLabel?: string | null; activeSmartFolderLabel?: string | null; activeCollectionLabel?: string | null; activeCollectionId?: number | null; activeStatusFilter?: string | null; filterTags?: string[] | null; currentView?: ViewType; folderLabel?: string | null; smartFolderLabel?: string | null; collectionLabel?: string | null; collectionId?: number | null; statusFilter?: string | null; view?: ViewType }): string {
  const folderLabel = state.activeFolderLabel ?? state.folderLabel;
  const smartFolderLabel = state.activeSmartFolderLabel ?? state.smartFolderLabel;
  const collectionLabel = state.activeCollectionLabel ?? state.collectionLabel;
  const collectionId = state.activeCollectionId ?? state.collectionId;
  const statusFilter = state.activeStatusFilter ?? state.statusFilter;
  const filterTags = state.filterTags;
  const view = state.currentView ?? state.view ?? 'images';
  if (filterTags && filterTags.length > 0) return filterTags.join(', ');
  if (folderLabel) return folderLabel;
  if (smartFolderLabel) return smartFolderLabel;
  if (collectionLabel) return collectionLabel;
  if (collectionId != null) return `Collection ${collectionId}`;
  if (statusFilter === 'inbox') return 'Inbox';
  if (statusFilter === 'uncategorized') return 'Uncategorized';
  if (statusFilter === 'trash') return 'Trash';
  if (statusFilter === 'untagged') return 'Untagged';
  if (statusFilter === 'random') return 'Random';
  return VIEW_LABELS[view];
}

export const useNavigationStore = create<NavigationState>((set, get) => ({
  currentView: 'images',
  activeSmartFolderId: null,
  activeFolderId: null,
  activeCollectionId: null,
  activeStatusFilter: null,
  filterTags: null,

  history: [{ view: 'images', smartFolderId: null, folderId: null, collectionId: null, statusFilter: null, filterTags: null, scrollTop: 0, loadedItemCount: 0, randomSeed: null }],
  historyIndex: 0,
  canGoBack: false,
  canGoForward: false,
  pendingScrollRestore: null, pendingLoadedItemCount: 0, pendingRandomSeed: null,

  navigateTo: (view, smartFolderId = null, folderId = null, statusFilter = null) => {
    const state = get();
    const trimmed = state.history.slice(0, state.historyIndex + 1);
    const entry: HistoryEntry = { view, smartFolderId, folderId, collectionId: null, statusFilter, filterTags: null, scrollTop: 0, loadedItemCount: 0, randomSeed: null };
    const newHistory = [...trimmed, entry];
    const newIndex = newHistory.length - 1;

    set({
      currentView: view,
      activeSmartFolderId: smartFolderId,
      activeFolderId: folderId,
      activeCollectionId: null,
      activeStatusFilter: statusFilter,
      filterTags: null,
      pendingScrollRestore: null, pendingLoadedItemCount: 0, pendingRandomSeed: null,
      history: newHistory,
      historyIndex: newIndex,
      canGoBack: newIndex > 0,
      canGoForward: false,
    });
  },

  goBack: () => {
    const state = get();
    if (state.historyIndex <= 0) return;
    const newIndex = state.historyIndex - 1;
    const entry = state.history[newIndex];

    set({
      currentView: entry.view,
      activeSmartFolderId: entry.smartFolderId,
      activeFolderId: entry.folderId,
      activeCollectionId: entry.collectionId,
      activeStatusFilter: entry.statusFilter,
      filterTags: entry.filterTags,
      historyIndex: newIndex,
      canGoBack: newIndex > 0,
      canGoForward: true,
      pendingScrollRestore: entry.scrollTop, pendingLoadedItemCount: entry.loadedItemCount, pendingRandomSeed: entry.randomSeed,
    });
  },

  goForward: () => {
    const state = get();
    if (state.historyIndex >= state.history.length - 1) return;
    const newIndex = state.historyIndex + 1;
    const entry = state.history[newIndex];

    set({
      currentView: entry.view,
      activeSmartFolderId: entry.smartFolderId,
      activeFolderId: entry.folderId,
      activeCollectionId: entry.collectionId,
      activeStatusFilter: entry.statusFilter,
      filterTags: entry.filterTags,
      historyIndex: newIndex,
      canGoBack: true,
      canGoForward: newIndex < state.history.length - 1,
      pendingScrollRestore: entry.scrollTop, pendingLoadedItemCount: entry.loadedItemCount, pendingRandomSeed: entry.randomSeed,
    });
  },

  saveScrollTop: (scrollTop, expectedHistoryIndex) => {
    set((state) => {
      if (
        expectedHistoryIndex != null
        && state.historyIndex !== expectedHistoryIndex
      ) {
        return state;
      }
      const history = [...state.history];
      if (history[state.historyIndex]) {
        history[state.historyIndex] = { ...history[state.historyIndex], scrollTop };
      }
      return { history };
    });
  },

  saveLoadedItemCount: (count: number) => {
    set((state) => {
      const history = [...state.history];
      if (history[state.historyIndex]) {
        history[state.historyIndex] = { ...history[state.historyIndex], loadedItemCount: count };
      }
      return { history };
    });
  },

  saveRandomSeed: (seed) => {
    set((state) => {
      const history = [...state.history];
      if (history[state.historyIndex]) {
        history[state.historyIndex] = { ...history[state.historyIndex], randomSeed: seed };
      }
      return { history };
    });
  },

  consumeScrollRestore: () => {
    const value = get().pendingScrollRestore;
    if (value != null) set({ pendingScrollRestore: null });
    return value;
  },

  setActiveFolderId: (folderId) => {
    set(() => ({
      activeFolderId: folderId,
      activeCollectionId: null,
    }));
  },

  navigateToFolder: (folder) => {
    const folderId = typeof folder === 'number' ? folder : folder.folder_id;
    get().navigateTo('images', null, folderId);
  },

  navigateToSmartFolder: (folder) => {
    get().navigateTo('images', folder.id ?? null, null);
  },

  navigateToCollection: (collection) => {
    const collectionId = typeof collection === 'number' ? collection : collection.id;
    if (typeof collection !== 'number' && collection.name) {
      useDomainStore.getState().rememberCollectionTitle(collection.id, collection.name);
    }
    const state = get();
    const trimmed = state.history.slice(0, state.historyIndex + 1);
    const entry: HistoryEntry = {
      view: 'images',
      smartFolderId: null,
      folderId: null,
      collectionId,
      statusFilter: null,
      filterTags: null,
      scrollTop: 0,
      loadedItemCount: 0,
      randomSeed: null,
    };
    const newHistory = [...trimmed, entry];
    const newIndex = newHistory.length - 1;

    set({
      currentView: 'images',
      activeSmartFolderId: null,
      activeFolderId: null,
      activeCollectionId: collectionId,
      activeStatusFilter: null,
      filterTags: null,
      pendingScrollRestore: null, pendingLoadedItemCount: 0, pendingRandomSeed: null,
      history: newHistory,
      historyIndex: newIndex,
      canGoBack: newIndex > 0,
      canGoForward: false,
    });
  },

  navigateToFilterTags: (tags) => {
    const state = get();
    const trimmed = state.history.slice(0, state.historyIndex + 1);
    const entry: HistoryEntry = { view: 'images', smartFolderId: null, folderId: null, collectionId: null, statusFilter: null, filterTags: tags, scrollTop: 0, loadedItemCount: 0, randomSeed: null };
    const newHistory = [...trimmed, entry];
    const newIndex = newHistory.length - 1;

    set({
      currentView: 'images',
      activeSmartFolderId: null,
      activeFolderId: null,
      activeCollectionId: null,
      activeStatusFilter: null,
      filterTags: tags,
      pendingScrollRestore: null, pendingLoadedItemCount: 0, pendingRandomSeed: null,
      history: newHistory,
      historyIndex: newIndex,
      canGoBack: newIndex > 0,
      canGoForward: false,
    });
  },
}));
