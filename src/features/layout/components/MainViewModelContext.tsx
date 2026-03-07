import { createContext, useContext, type ReactNode } from 'react';
import type { AppSettings } from '../../state/settingsStore';
import type { SmartFolderPredicate } from '../smart-folders/types';
import type { GridViewMode, DetailViewControls, DetailViewState } from '#features/grid/components';
import type { MasonryImageItem, SelectionQuerySpec } from '#features/grid/types';
import type { FlowResultEntry } from '#features/subscriptions/components';

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

type MainViewFlowsState = {
  activeFlowId?: string;
  flowLastResults: Record<number, FlowResultEntry>;
  setFlowLastResults: (next: Record<number, FlowResultEntry>) => void;
  flowRefreshToken?: number;
  onOpenCreateFlowModal: () => void;
};

export type MainViewModel = {
  navigation: MainViewNavigationState;
  grid: MainViewGridState;
  gridActions: MainViewGridActions;
  selection: MainViewSelectionState;
  flows: MainViewFlowsState;
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

export function useMainViewFlowsState(): MainViewFlowsState {
  return useMainViewModel().flows;
}
