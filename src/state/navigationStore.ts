/**
 * Navigation store — single source of truth for app navigation state.
 *
 * Replaces the 10+ useState calls in App.tsx for view type, active folder,
 * active smart folder, history stack, etc.
 */

import { create } from 'zustand';
import type { SmartFolder } from '#features/smart-folders/types';

export type ViewType = 'images' | 'collections' | 'subscriptions' | 'duplicates' | 'tags';

export const VIEW_LABELS: Record<ViewType, string> = {
  images: 'All Active',
  collections: 'Albums',
  subscriptions: 'Subscriptions',
  duplicates: 'Duplicates',
  tags: 'Tags',
};

export interface ActiveFolder {
  folder_id: number;
  name: string;
}

export interface ActiveSubscriptionGroup {
  id: string;
  name: string;
}

export interface ActiveCollection {
  id: number;
  name: string;
}

interface HistoryEntry {
  view: ViewType;
  smartFolderId: string | null;
  folder: ActiveFolder | null;
  collection: ActiveCollection | null;
  subscriptionGroup: ActiveSubscriptionGroup | null;
  statusFilter: string | null;
  filterTags: string[] | null;
  scrollTop: number;
}

interface NavigationState {
  // Current state
  currentView: ViewType;
  activeSmartFolderId: string | null;
  activeFolder: ActiveFolder | null;
  activeCollection: ActiveCollection | null;
  activeSubscriptionGroup: ActiveSubscriptionGroup | null;
  activeStatusFilter: string | null;
  filterTags: string[] | null;

  // History
  history: HistoryEntry[];
  historyIndex: number;
  canGoBack: boolean;
  canGoForward: boolean;

  // Scroll restore for back/forward navigation
  pendingScrollRestore: number | null;

  // Actions
  navigateTo: (view: ViewType, smartFolderId?: string | null, folder?: ActiveFolder | null, statusFilter?: string | null) => void;
  goBack: () => void;
  goForward: () => void;
  /** Save current scroll position to the current history entry */
  saveScrollTop: (scrollTop: number, expectedHistoryIndex?: number) => void;
  /** Consume the pending scroll restore value (returns it and clears it) */
  consumeScrollRestore: () => number | null;
  setActiveFolder: (folder: ActiveFolder | null) => void;
  /** Navigate to a folder (sets view to 'images', clears smart folder) */
  navigateToFolder: (folder: ActiveFolder) => void;
  /** Navigate to a smart folder (sets view to 'images', clears folder) */
  navigateToSmartFolder: (folder: SmartFolder) => void;
  /** Navigate to a collection drill-down session (images view scoped to collection members). */
  navigateToCollection: (collection: ActiveCollection) => void;
  /** Navigate to a subscription group (sets view to 'subscriptions', clears folder/smart folder) */
  navigateToSubscriptionGroup: (subscriptionGroup: ActiveSubscriptionGroup) => void;
  /** Navigate to images view filtered by specific tags */
  navigateToFilterTags: (tags: string[]) => void;

}

export function deriveNavigationTitle(state: { activeFolder?: ActiveFolder | null; activeSmartFolder?: SmartFolder | null; activeCollection?: ActiveCollection | null; activeSubscriptionGroup?: ActiveSubscriptionGroup | null; activeStatusFilter?: string | null; filterTags?: string[] | null; currentView?: ViewType; folder?: ActiveFolder | null; smartFolder?: SmartFolder | null; collection?: ActiveCollection | null; subscriptionGroup?: ActiveSubscriptionGroup | null; statusFilter?: string | null; view?: ViewType }): string {
  const folder = state.activeFolder ?? state.folder;
  const smartFolder = state.activeSmartFolder ?? state.smartFolder;
  const collection = state.activeCollection ?? state.collection;
  const subscriptionGroup = state.activeSubscriptionGroup ?? state.subscriptionGroup;
  const statusFilter = state.activeStatusFilter ?? state.statusFilter;
  const filterTags = state.filterTags;
  const view = state.currentView ?? state.view ?? 'images';
  if (filterTags && filterTags.length > 0) return filterTags.join(', ');
  if (folder) return folder.name;
  if (smartFolder) return smartFolder.name;
  if (collection) return collection.name;
  if (subscriptionGroup) return subscriptionGroup.name;
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
  activeFolder: null,
  activeCollection: null,
  activeSubscriptionGroup: null,
  activeStatusFilter: null,
  filterTags: null,

  history: [{ view: 'images', smartFolderId: null, folder: null, collection: null, subscriptionGroup: null, statusFilter: null, filterTags: null, scrollTop: 0 }],
  historyIndex: 0,
  canGoBack: false,
  canGoForward: false,
  pendingScrollRestore: null,


  navigateTo: (view, smartFolderId = null, folder = null, statusFilter = null) => {
    const state = get();
    const trimmed = state.history.slice(0, state.historyIndex + 1);
    const entry: HistoryEntry = { view, smartFolderId, folder, collection: null, subscriptionGroup: null, statusFilter, filterTags: null, scrollTop: 0 };
    const newHistory = [...trimmed, entry];
    const newIndex = newHistory.length - 1;

    set({
      currentView: view,
      activeSmartFolderId: smartFolderId,
      activeFolder: folder,
      activeCollection: null,
      activeSubscriptionGroup: null,
      activeStatusFilter: statusFilter,
      filterTags: null,
      pendingScrollRestore: null,
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
      activeFolder: entry.folder,
      activeCollection: entry.collection,
      activeSubscriptionGroup: entry.subscriptionGroup,
      activeStatusFilter: entry.statusFilter,
      filterTags: entry.filterTags,
      historyIndex: newIndex,
      canGoBack: newIndex > 0,
      canGoForward: true,
      pendingScrollRestore: entry.scrollTop,
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
      activeFolder: entry.folder,
      activeCollection: entry.collection,
      activeSubscriptionGroup: entry.subscriptionGroup,
      activeStatusFilter: entry.statusFilter,
      filterTags: entry.filterTags,
      historyIndex: newIndex,
      canGoBack: true,
      canGoForward: newIndex < state.history.length - 1,
      pendingScrollRestore: entry.scrollTop,
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

  consumeScrollRestore: () => {
    const value = get().pendingScrollRestore;
    if (value != null) set({ pendingScrollRestore: null });
    return value;
  },

  setActiveFolder: (folder) => {
    set(() => ({
      activeFolder: folder,
      activeCollection: null,
      activeSubscriptionGroup: null,
    }));
  },

  navigateToFolder: (folder) => {
    get().navigateTo('images', null, folder);
  },

  navigateToSmartFolder: (folder) => {
    get().navigateTo('images', folder.id ?? null, null);
  },

  navigateToCollection: (collection) => {
    const state = get();
    const trimmed = state.history.slice(0, state.historyIndex + 1);
    const entry: HistoryEntry = {
      view: 'images',
      smartFolderId: null,
      folder: null,
      collection,
      subscriptionGroup: null,
      statusFilter: null,
      filterTags: null,
      scrollTop: 0,
    };
    const newHistory = [...trimmed, entry];
    const newIndex = newHistory.length - 1;

    set({
      currentView: 'images',
      activeSmartFolderId: null,
      activeFolder: null,
      activeCollection: collection,
      activeSubscriptionGroup: null,
      activeStatusFilter: null,
      filterTags: null,
      pendingScrollRestore: null,
      history: newHistory,
      historyIndex: newIndex,
      canGoBack: newIndex > 0,
      canGoForward: false,
    });
  },

  navigateToSubscriptionGroup: (subscriptionGroup) => {
    const state = get();
    const trimmed = state.history.slice(0, state.historyIndex + 1);
    const entry: HistoryEntry = { view: 'subscriptions', smartFolderId: null, folder: null, collection: null, subscriptionGroup, statusFilter: null, filterTags: null, scrollTop: 0 };
    const newHistory = [...trimmed, entry];
    const newIndex = newHistory.length - 1;

    set({
      currentView: 'subscriptions',
      activeSmartFolderId: null,
      activeFolder: null,
      activeCollection: null,
      activeSubscriptionGroup: subscriptionGroup,
      activeStatusFilter: null,
      filterTags: null,
      pendingScrollRestore: null,
      history: newHistory,
      historyIndex: newIndex,
      canGoBack: newIndex > 0,
      canGoForward: false,
    });
  },

  navigateToFilterTags: (tags) => {
    const state = get();
    const trimmed = state.history.slice(0, state.historyIndex + 1);
    const entry: HistoryEntry = { view: 'images', smartFolderId: null, folder: null, collection: null, subscriptionGroup: null, statusFilter: null, filterTags: tags, scrollTop: 0 };
    const newHistory = [...trimmed, entry];
    const newIndex = newHistory.length - 1;

    set({
      currentView: 'images',
      activeSmartFolderId: null,
      activeFolder: null,
      activeCollection: null,
      activeSubscriptionGroup: null,
      activeStatusFilter: null,
      filterTags: tags,
      pendingScrollRestore: null,
      history: newHistory,
      historyIndex: newIndex,
      canGoBack: newIndex > 0,
      canGoForward: false,
    });
  },
}));
