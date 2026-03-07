import { Collections } from '#features/collections/components';
import { FlowsWorking, type FlowResultEntry, CreateFlowModal } from '#features/subscriptions/components';
import { TagManager } from '#features/tags/components';
import { DuplicateManager } from '#features/duplicates/components';
import { ImageGrid } from '#features/grid/components';
import { useMainViewFlowsState, useMainViewGridActions, useMainViewGridState, useMainViewNavigationState, useMainViewSelectionState } from './MainViewModelContext';
import styles from '../../../app/App.module.css';

export function MainViewRouter() {
  const navigation = useMainViewNavigationState();
  const grid = useMainViewGridState();
  const gridActions = useMainViewGridActions();
  const selection = useMainViewSelectionState();
  const flows = useMainViewFlowsState();

  switch (navigation.currentView) {
    case 'images':
      return (
        <div className={styles.frame}>
          <ImageGrid
            searchTags={grid.searchTags}
            smartFolderPredicate={navigation.activeSmartFolderPredicate}
            smartFolderSortField={navigation.activeSmartFolderSortField}
            smartFolderSortOrder={navigation.activeSmartFolderSortOrder}
            folderId={navigation.activeFolderId}
            collectionEntityId={navigation.activeCollectionId}
            filterFolderIds={grid.filterFolderIds}
            excludedFilterFolderIds={grid.excludedFilterFolderIds}
            folderMatchMode={grid.folderMatchMode}
            statusFilter={navigation.activeStatusFilter}
            viewMode={grid.viewMode}
            targetSize={grid.targetSize}
            onViewModeChange={gridActions.onViewModeChange}
            sortField={grid.sortField}
            sortOrder={grid.sortOrder}
            onSortFieldChange={gridActions.onSortFieldChange}
            onSortOrderChange={gridActions.onSortOrderChange}
            onContainerWidthChange={gridActions.onContainerWidthChange}
            refreshTrigger={grid.filterRefreshTrigger}
            onSelectedImagesChange={selection.onSelectedImagesChange}
            onSelectionSummarySpecChange={selection.onSelectionSummarySpecChange}
            selectedScopeCount={grid.selectedScopeCount}
            onDetailViewStateChange={selection.onDetailViewStateChange}
            ratingMin={grid.ratingFilter}
            mimePrefixes={grid.mimePrefixes}
            colorHex={grid.colorHex}
            colorAccuracy={grid.colorAccuracy}
            searchText={grid.searchText || grid.filterSearchText}
            excludedSearchTags={grid.excludedSearchTags}
            tagMatchMode={grid.tagMatchMode}
            externalFreeze={false}
            onScopeTransitionMidpoint={gridActions.onScopeTransitionMidpoint}
          />
        </div>
      );
    case 'collections':
      return <div className={styles.frame}><Collections /></div>;
    case 'flows':
      return (
        <div className={styles.frame}>
          <FlowsWorking
            flowId={flows.activeFlowId}
            lastResults={flows.flowLastResults}
            onLastResultsChange={flows.setFlowLastResults}
            onOpenCreateModal={flows.onOpenCreateFlowModal}
            refreshToken={flows.flowRefreshToken}
          />
        </div>
      );
    case 'tags':
      return <div className={styles.frame}><TagManager /></div>;
    case 'duplicates':
      return <div className={styles.frame}><DuplicateManager /></div>;
    default:
      return null;
  }
}

export { CreateFlowModal };
export type { FlowResultEntry };
