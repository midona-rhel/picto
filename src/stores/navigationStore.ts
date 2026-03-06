/**
 * Navigation store — single source of truth for app navigation state.
 *
 * Replaces the 10+ useState calls in App.tsx for view type, active folder,
 * active smart folder, history stack, etc.
 */

import { create } from 'zustand';
import type { SmartFolder } from '#features/smart-folders/types';

export type ViewType = 'images' | 'collections' | 'flows' | 'duplicates' | 'tags';

export const VIEW_LABELS: Record<ViewType, string> = {
  images: 'All Images',
  collections: 'Albums',
  flows: 'Flows',
  duplicates: 'Duplicates',
  tags: 'Tags',
};

export interface ActiveFolder {
  folder_id: number;
  name: string;
}

export interface ActiveFlow {
  id: string;
  name: string;
}

export interface ActiveCollection {
  id: number;
  name: string;
}

interface HistoryEntry {
  view: ViewType;
  smartFolder: SmartFolder | null;
  folder: ActiveFolder | null;
  collection: ActiveCollection | null;
  flow: ActiveFlow | null;
  statusFilter: string | null;
  filterTags: string[] | null;
}

interface NavigationState {
  // Current state
  currentView: ViewType;
  activeSmartFolder: SmartFolder | null;
  activeFolder: ActiveFolder | null;
  activeCollection: ActiveCollection | null;
  activeFlow: ActiveFlow | null;
  activeStatusFilter: string | null;
  filterTags: string[] | null;

  // History
  history: HistoryEntry[];
  historyIndex: number;
  canGoBack: boolean;
  canGoForward: boolean;

  // Actions
  navigateTo: (view: ViewType, smartFolder?: SmartFolder | null, folder?: ActiveFolder | null, statusFilter?: string | null) => void;
  goBack: () => void;
  goForward: () => void;
  setActiveFolder: (folder: ActiveFolder | null) => void;
  setActiveSmartFolder: (folder: SmartFolder | null) => void;
  /** Navigate to a folder (sets view to 'images', clears smart folder) */
  navigateToFolder: (folder: ActiveFolder) => void;
  /** Navigate to a smart folder (sets view to 'images', clears folder) */
  navigateToSmartFolder: (folder: SmartFolder) => void;
  /** Navigate to a collection drill-down session (images view scoped to collection members). */
  navigateToCollection: (collection: ActiveCollection) => void;
  /** Navigate to a flow (sets view to 'flows', clears folder/smart folder) */
  navigateToFlow: (flow: ActiveFlow) => void;
  /** Navigate to images view filtered by specific tags */
  navigateToFilterTags: (tags: string[]) => void;

  /** Title computed from current navigation state */
  titlebarTitle: string;
}

function computeTitle(state: { activeFolder?: ActiveFolder | null; activeSmartFolder?: SmartFolder | null; activeCollection?: ActiveCollection | null; activeFlow?: ActiveFlow | null; activeStatusFilter?: string | null; filterTags?: string[] | null; currentView?: ViewType; folder?: ActiveFolder | null; smartFolder?: SmartFolder | null; collection?: ActiveCollection | null; flow?: ActiveFlow | null; statusFilter?: string | null; view?: ViewType }): string {
  const folder = state.activeFolder ?? state.folder;
  const smartFolder = state.activeSmartFolder ?? state.smartFolder;
  const collection = state.activeCollection ?? state.collection;
  const flow = state.activeFlow ?? state.flow;
  const statusFilter = state.activeStatusFilter ?? state.statusFilter;
  const filterTags = state.filterTags;
  const view = state.currentView ?? state.view ?? 'images';
  if (filterTags && filterTags.length > 0) return filterTags.join(', ');
  if (folder) return folder.name;
  if (smartFolder) return smartFolder.name;
  if (collection) return collection.name;
  if (flow) return flow.name;
  if (statusFilter === 'inbox') return 'Inbox';
  if (statusFilter === 'uncategorized') return 'Uncategorized';
  if (statusFilter === 'trash') return 'Trash';
  if (statusFilter === 'untagged') return 'Untagged';
  if (statusFilter === 'recently_viewed') return 'Recently Viewed';
  if (statusFilter === 'random') return 'Random';
  return VIEW_LABELS[view];
}

export const useNavigationStore = create<NavigationState>((set, get) => ({
  currentView: 'images',
  activeSmartFolder: null,
  activeFolder: null,
  activeCollection: null,
  activeFlow: null,
  activeStatusFilter: null,
  filterTags: null,

  history: [{ view: 'images', smartFolder: null, folder: null, collection: null, flow: null, statusFilter: null, filterTags: null }],
  historyIndex: 0,
  canGoBack: false,
  canGoForward: false,

  titlebarTitle: VIEW_LABELS.images,

  navigateTo: (view, smartFolder = null, folder = null, statusFilter = null) => {
    const state = get();
    const trimmed = state.history.slice(0, state.historyIndex + 1);
    const entry: HistoryEntry = { view, smartFolder, folder, collection: null, flow: null, statusFilter, filterTags: null };
    const newHistory = [...trimmed, entry];
    const newIndex = newHistory.length - 1;

    set({
      currentView: view,
      activeSmartFolder: smartFolder,
      activeFolder: folder,
      activeCollection: null,
      activeFlow: null,
      activeStatusFilter: statusFilter,
      filterTags: null,
      history: newHistory,
      historyIndex: newIndex,
      canGoBack: newIndex > 0,
      canGoForward: false,
      titlebarTitle: computeTitle({ activeFolder: folder, activeSmartFolder: smartFolder, activeCollection: null, activeFlow: null, activeStatusFilter: statusFilter, currentView: view }),
    });
  },

  goBack: () => {
    const state = get();
    if (state.historyIndex <= 0) return;
    const newIndex = state.historyIndex - 1;
    const entry = state.history[newIndex];

    set({
      currentView: entry.view,
      activeSmartFolder: entry.smartFolder,
      activeFolder: entry.folder,
      activeCollection: entry.collection,
      activeFlow: entry.flow,
      activeStatusFilter: entry.statusFilter,
      filterTags: entry.filterTags,
      historyIndex: newIndex,
      canGoBack: newIndex > 0,
      canGoForward: true,
      titlebarTitle: computeTitle(entry),
    });
  },

  goForward: () => {
    const state = get();
    if (state.historyIndex >= state.history.length - 1) return;
    const newIndex = state.historyIndex + 1;
    const entry = state.history[newIndex];

    set({
      currentView: entry.view,
      activeSmartFolder: entry.smartFolder,
      activeFolder: entry.folder,
      activeCollection: entry.collection,
      activeFlow: entry.flow,
      activeStatusFilter: entry.statusFilter,
      filterTags: entry.filterTags,
      historyIndex: newIndex,
      canGoBack: true,
      canGoForward: newIndex < state.history.length - 1,
      titlebarTitle: computeTitle(entry),
    });
  },

  setActiveFolder: (folder) => {
    set((state) => ({
      activeFolder: folder,
      activeCollection: null,
      titlebarTitle: computeTitle({ ...state, activeFolder: folder }),
    }));
  },

  setActiveSmartFolder: (folder) => {
    set((state) => ({
      activeSmartFolder: folder,
      activeCollection: null,
      titlebarTitle: computeTitle({ ...state, activeSmartFolder: folder }),
    }));
  },

  navigateToFolder: (folder) => {
    get().navigateTo('images', null, folder);
  },

  navigateToSmartFolder: (folder) => {
    get().navigateTo('images', folder, null);
  },

  navigateToCollection: (collection) => {
    const state = get();
    const trimmed = state.history.slice(0, state.historyIndex + 1);
    const entry: HistoryEntry = {
      view: 'images',
      smartFolder: null,
      folder: null,
      collection,
      flow: null,
      statusFilter: null,
      filterTags: null,
    };
    const newHistory = [...trimmed, entry];
    const newIndex = newHistory.length - 1;

    set({
      currentView: 'images',
      activeSmartFolder: null,
      activeFolder: null,
      activeCollection: collection,
      activeFlow: null,
      activeStatusFilter: null,
      filterTags: null,
      history: newHistory,
      historyIndex: newIndex,
      canGoBack: newIndex > 0,
      canGoForward: false,
      titlebarTitle: computeTitle({ activeCollection: collection, currentView: 'images' }),
    });
  },

  navigateToFlow: (flow) => {
    const state = get();
    const trimmed = state.history.slice(0, state.historyIndex + 1);
    const entry: HistoryEntry = { view: 'flows', smartFolder: null, folder: null, collection: null, flow, statusFilter: null, filterTags: null };
    const newHistory = [...trimmed, entry];
    const newIndex = newHistory.length - 1;

    set({
      currentView: 'flows',
      activeSmartFolder: null,
      activeFolder: null,
      activeCollection: null,
      activeFlow: flow,
      activeStatusFilter: null,
      filterTags: null,
      history: newHistory,
      historyIndex: newIndex,
      canGoBack: newIndex > 0,
      canGoForward: false,
      titlebarTitle: computeTitle({ activeFlow: flow, currentView: 'flows' }),
    });
  },

  navigateToFilterTags: (tags) => {
    const state = get();
    const trimmed = state.history.slice(0, state.historyIndex + 1);
    const entry: HistoryEntry = { view: 'images', smartFolder: null, folder: null, collection: null, flow: null, statusFilter: null, filterTags: tags };
    const newHistory = [...trimmed, entry];
    const newIndex = newHistory.length - 1;

    set({
      currentView: 'images',
      activeSmartFolder: null,
      activeFolder: null,
      activeCollection: null,
      activeFlow: null,
      activeStatusFilter: null,
      filterTags: tags,
      history: newHistory,
      historyIndex: newIndex,
      canGoBack: newIndex > 0,
      canGoForward: false,
      titlebarTitle: computeTitle({ filterTags: tags, currentView: 'images' }),
    });
  },
}));
