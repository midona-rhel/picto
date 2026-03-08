import { createContext, useContext, type ReactNode } from 'react';
import type { AppSettings } from '../../../state/settingsStore';
import type { SmartFolderPredicate } from '../../smart-folders/components/types';
import type { GridViewMode, DetailViewControls, DetailViewState } from '#features/grid/components';
import type { MasonryImageItem, SelectionQuerySpec } from '#features/grid/types';
import type { ViewerHostController } from '#features/viewer/hooks/useViewerHost';
type MainViewNavigationState = {
  currentView: string;
  activeSmartFolderPredicate?: SmartFolderPredicate;
  activeSmartFolderSortField?: string;
  activeSmartFolderSortOrder?: string;
  activeFolderId: number | null;
  activeCollectionId: number | null;
  activeStatusFilter: string | null;
};

type MainViewGridState = {
  viewMode: GridViewMode;
  targetSize: number;
  sortField: AppSettings['gridSortField'];
  sortOrder: AppSettings['gridSortOrder'];
  searchTags: string[];
  excludedSearchTags: string[];
  tagMatchMode: 'all' | 'any' | 'exact';
  searchText: string;
  filterSearchText: string;
  filterFolderIds: number[] | null;
  excludedFilterFolderIds: number[] | null;
  folderMatchMode: 'all' | 'any' | 'exact';
  ratingFilter: number | null;
  mimePrefixes: string[] | null;
  colorHex: string | null;
  colorAccuracy: number;
  filterRefreshTrigger: number;
  selectedScopeCount: number | null;
};

type MainViewGridActions = {
  onContainerWidthChange: (width: number) => void;
  onViewModeChange: (mode: GridViewMode) => void;
  onSortFieldChange: (field: string) => void;
  onSortOrderChange: (order: string) => void;
  onScopeTransitionMidpoint: () => void;
};

type MainViewSelectionState = {
  onSelectedImagesChange: (images: MasonryImageItem[]) => void;
  onSelectionSummarySpecChange: (spec: SelectionQuerySpec | null) => void;
  onDetailViewStateChange: (state: DetailViewState | null, controls: DetailViewControls | null) => void;
};

type MainViewSubscriptionsState = {
  subscriptionRefreshToken?: number;
  onOpenCreateSubscriptionGroupModal: () => void;
};

type MainViewViewerState = ViewerHostController;

export type MainViewModel = {
  navigation: MainViewNavigationState;
  grid: MainViewGridState;
  gridActions: MainViewGridActions;
  selection: MainViewSelectionState;
  subscriptions: MainViewSubscriptionsState;
  viewer: MainViewViewerState;
};

const MainViewModelContext = createContext<MainViewModel | null>(null);

type MainViewModelProviderProps = {
  value: MainViewModel;
  children: ReactNode;
};

export function MainViewModelProvider({ value, children }: MainViewModelProviderProps) {
  return <MainViewModelContext.Provider value={value}>{children}</MainViewModelContext.Provider>;
}

export function useMainViewModel(): MainViewModel {
  const value = useContext(MainViewModelContext);
  if (!value) {
    throw new Error('useMainViewModel must be used within MainViewModelProvider');
  }
  return value;
}

export function useMainViewNavigationState(): MainViewNavigationState {
  return useMainViewModel().navigation;
}

export function useMainViewGridState(): MainViewGridState {
  return useMainViewModel().grid;
}

export function useMainViewGridActions(): MainViewGridActions {
  return useMainViewModel().gridActions;
}

export function useMainViewSelectionState(): MainViewSelectionState {
  return useMainViewModel().selection;
}

export function useMainViewSubscriptionsState(): MainViewSubscriptionsState {
  return useMainViewModel().subscriptions;
}

export function useMainViewViewerState(): MainViewViewerState {
  return useMainViewModel().viewer;
}
