import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi, beforeEach } from 'vitest';
import { MainViewRouter } from '../MainViewRouter';
import { MainViewModelProvider, type MainViewModel } from '../MainViewModelContext';

const mocks = vi.hoisted(() => ({
  imageGrid: vi.fn(),
  flowsWorking: vi.fn(),
}));

vi.mock('#features/collections/components', () => ({
  Collections: () => <div data-testid="view-collections">collections-view</div>,
}));

vi.mock('#features/subscriptions/components', () => ({
  FlowsWorking: (props: unknown) => {
    mocks.flowsWorking(props);
    return <div data-testid="view-flows">flows-view</div>;
  },
  CreateFlowModal: () => null,
}));

vi.mock('#features/tags/components', () => ({
  TagManager: () => <div data-testid="view-tags">tags-view</div>,
}));

vi.mock('#features/duplicates/components', () => ({
  DuplicateManager: () => <div data-testid="view-duplicates">duplicates-view</div>,
}));

vi.mock('#features/grid/components', () => ({
  ImageGrid: (props: unknown) => {
    mocks.imageGrid(props);
    return <div data-testid="view-images">images-view</div>;
  },
}));

function createMainViewModel(overrides?: Partial<MainViewModel['navigation']>): MainViewModel {
  return {
    navigation: {
      currentView: 'images',
      activeSmartFolderPredicate: undefined,
      activeSmartFolderSortField: undefined,
      activeSmartFolderSortOrder: undefined,
      activeFolderId: null,
      activeCollectionId: null,
      activeStatusFilter: null,
      ...overrides,
    },
    grid: {
      viewMode: 'grid',
      targetSize: 220,
      sortField: 'imported_at',
      sortOrder: 'desc',
      searchTags: ['alpha'],
      excludedSearchTags: ['beta'],
      tagMatchMode: 'all',
      searchText: '',
      filterSearchText: '',
      filterFolderIds: null,
      excludedFilterFolderIds: null,
      folderMatchMode: 'all',
      ratingFilter: null,
      mimePrefixes: null,
      colorHex: null,
      colorAccuracy: 95,
      filterRefreshTrigger: 0,
      selectedScopeCount: null,
    },
    gridActions: {
      onContainerWidthChange: () => {},
      onViewModeChange: () => {},
      onSortFieldChange: () => {},
      onSortOrderChange: () => {},
      onScopeTransitionMidpoint: () => {},
    },
    selection: {
      onSelectedImagesChange: () => {},
      onSelectionSummarySpecChange: () => {},
      onDetailViewStateChange: () => {},
    },
    flows: {
      activeFlowId: 'flow-1',
      flowLastResults: {},
      setFlowLastResults: () => {},
      flowRefreshToken: 7,
      onOpenCreateFlowModal: () => {},
    },
  };
}

function renderRouter(model: MainViewModel) {
  return render(
    <MainViewModelProvider value={model}>
      <MainViewRouter />
    </MainViewModelProvider>,
  );
}

describe('MainViewRouter', () => {
  beforeEach(() => {
    mocks.imageGrid.mockClear();
    mocks.flowsWorking.mockClear();
  });

  it('renders image grid and passes scope/query state from provider', () => {
    const model = createMainViewModel({
      currentView: 'images',
      activeFolderId: 42,
      activeStatusFilter: 'inbox',
    });

    renderRouter(model);

    expect(screen.getByTestId('view-images')).toBeTruthy();
    expect(mocks.imageGrid).toHaveBeenCalledTimes(1);
    expect(mocks.imageGrid.mock.calls[0][0]).toMatchObject({
      folderId: 42,
      statusFilter: 'inbox',
      viewMode: 'grid',
      sortField: 'imported_at',
      sortOrder: 'desc',
      searchTags: ['alpha'],
      excludedSearchTags: ['beta'],
    });
  });

  it('routes to feature views based on provider navigation state', () => {
    renderRouter(createMainViewModel({ currentView: 'collections' }));
    expect(screen.getByTestId('view-collections')).toBeTruthy();

    renderRouter(createMainViewModel({ currentView: 'tags' }));
    expect(screen.getByTestId('view-tags')).toBeTruthy();

    renderRouter(createMainViewModel({ currentView: 'duplicates' }));
    expect(screen.getByTestId('view-duplicates')).toBeTruthy();
  });

  it('renders flows view and forwards flow model state', () => {
    const model = createMainViewModel({ currentView: 'flows' });
    renderRouter(model);

    expect(screen.getByTestId('view-flows')).toBeTruthy();
    expect(mocks.flowsWorking).toHaveBeenCalledTimes(1);
    expect(mocks.flowsWorking.mock.calls[0][0]).toMatchObject({
      flowId: 'flow-1',
      refreshToken: 7,
    });
  });
});
