import { Collections } from '../Collections';
import { FlowsWorking, type FlowResultEntry, CreateFlowModal } from '../FlowsWorking';
import { TagManager } from '../TagManager';
import { DuplicateManager } from '../DuplicateManager';
import { ImageGrid } from '../image-grid/ImageGrid';
import type { GridViewMode } from '../image-grid/ImageGrid';
import type { DetailViewControls, DetailViewState } from '../image-grid/DetailView';
import type { MasonryImageItem } from '../image-grid/shared';
import type { SelectionQuerySpec } from '../image-grid/metadataPrefetch';
import type { SmartFolderPredicate } from '../smart-folders/types';
import type { AppSettings } from '../../stores/settingsStore';
import styles from '../../App.module.css';

type MainViewRouterProps = {
  currentView: string;
  activeSmartFolderPredicate?: SmartFolderPredicate;
  activeSmartFolderSortField?: string;
  activeSmartFolderSortOrder?: string;
  activeFolderId: number | null;
  activeCollectionId: number | null;
  filterFolderIds: number[] | null;
  excludedFilterFolderIds: number[] | null;
  folderMatchMode: 'all' | 'any' | 'exact';
  activeStatusFilter: string | null;
  gridViewMode: GridViewMode;
  gridTargetSize: number;
  gridSortField: AppSettings['gridSortField'];
  gridSortOrder: AppSettings['gridSortOrder'];
  effectiveSearchTags: string[];
  excludedSearchTags: string[];
  tagMatchMode: 'all' | 'any' | 'exact';
  searchText: string;
  filterSearchText: string;
  ratingFilter: number | null;
  mimePrefixes: string[] | null;
  colorHex: string | null;
  colorAccuracy: number;
  filterRefreshTrigger: number;
  selectedScopeCount: number | null;
  activeFlowId?: string;
  flowLastResults: Record<number, FlowResultEntry>;
  setFlowLastResults: (next: Record<number, FlowResultEntry>) => void;
  flowRefreshToken?: number;
  onOpenCreateFlowModal: () => void;
  onContainerWidthChange: (width: number) => void;
  onViewModeChange: (mode: GridViewMode) => void;
  onSortFieldChange: (field: string) => void;
  onSortOrderChange: (order: string) => void;
  onSelectedImagesChange: (images: MasonryImageItem[]) => void;
  onSelectionSummarySpecChange: (spec: SelectionQuerySpec | null) => void;
  onDetailViewStateChange: (state: DetailViewState | null, controls: DetailViewControls | null) => void;
  onScopeTransitionMidpoint: () => void;
};

export function MainViewRouter(props: MainViewRouterProps) {
  const {
    currentView,
    activeSmartFolderPredicate,
    activeSmartFolderSortField,
    activeSmartFolderSortOrder,
    activeFolderId,
    activeCollectionId,
    filterFolderIds,
    excludedFilterFolderIds,
    folderMatchMode,
    activeStatusFilter,
    gridViewMode,
    gridTargetSize,
    gridSortField,
    gridSortOrder,
    effectiveSearchTags,
    excludedSearchTags,
    tagMatchMode,
    searchText,
    filterSearchText,
    ratingFilter,
    mimePrefixes,
    colorHex,
    colorAccuracy,
    filterRefreshTrigger,
    selectedScopeCount,
    activeFlowId,
    flowLastResults,
    setFlowLastResults,
    flowRefreshToken,
    onOpenCreateFlowModal,
    onContainerWidthChange,
    onViewModeChange,
    onSortFieldChange,
    onSortOrderChange,
    onSelectedImagesChange,
    onSelectionSummarySpecChange,
    onDetailViewStateChange,
    onScopeTransitionMidpoint,
  } = props;

  switch (currentView) {
    case 'images':
      return (
        <div className={styles.frame}>
          <ImageGrid
            searchTags={effectiveSearchTags}
            smartFolderPredicate={activeSmartFolderPredicate}
            smartFolderSortField={activeSmartFolderSortField}
            smartFolderSortOrder={activeSmartFolderSortOrder}
            folderId={activeFolderId}
            collectionEntityId={activeCollectionId}
            filterFolderIds={filterFolderIds}
            excludedFilterFolderIds={excludedFilterFolderIds}
            folderMatchMode={folderMatchMode}
            statusFilter={activeStatusFilter}
            viewMode={gridViewMode}
            targetSize={gridTargetSize}
            onViewModeChange={onViewModeChange}
            sortField={gridSortField}
            sortOrder={gridSortOrder}
            onSortFieldChange={onSortFieldChange}
            onSortOrderChange={onSortOrderChange}
            onContainerWidthChange={onContainerWidthChange}
            refreshTrigger={filterRefreshTrigger}
            onSelectedImagesChange={onSelectedImagesChange}
            onSelectionSummarySpecChange={onSelectionSummarySpecChange}
            selectedScopeCount={selectedScopeCount}
            onDetailViewStateChange={onDetailViewStateChange}
            ratingMin={ratingFilter}
            mimePrefixes={mimePrefixes}
            colorHex={colorHex}
            colorAccuracy={colorAccuracy}
            searchText={searchText || filterSearchText}
            excludedSearchTags={excludedSearchTags}
            tagMatchMode={tagMatchMode}
            externalFreeze={false}
            onScopeTransitionMidpoint={onScopeTransitionMidpoint}
          />
        </div>
      );
    case 'collections':
      return <div className={styles.frame}><Collections /></div>;
    case 'flows':
      return (
        <div className={styles.frame}>
          <FlowsWorking
            flowId={activeFlowId}
            lastResults={flowLastResults}
            onLastResultsChange={setFlowLastResults}
            onOpenCreateModal={onOpenCreateFlowModal}
            refreshToken={flowRefreshToken}
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
