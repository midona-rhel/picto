import { Collections } from '#features/collections/components';
import { SubscriptionGroupsPanel, CreateSubscriptionGroupModal } from '#features/subscriptions/components';
import { TagManager } from '#features/tags/components';
import { DuplicateManager } from '#features/duplicates/components';
import { GridRoot } from '#features/grid/GridRoot';
import { useMainViewSubscriptionsState, useMainViewGridActions, useMainViewGridState, useMainViewNavigationState, useMainViewViewerState } from './MainViewModelContext';
import { MainViewProgressBar } from './MainViewProgressBar';
import styles from '../../../app/App.module.css';

export function MainViewRouter() {
  const navigation = useMainViewNavigationState();
  const grid = useMainViewGridState();
  const gridActions = useMainViewGridActions();
  const subscriptions = useMainViewSubscriptionsState();
  const viewer = useMainViewViewerState();

  switch (navigation.currentView) {
    case 'images':
      return (
        <div className={styles.frame}>
          <GridRoot
            viewMode={grid.viewMode}
            targetSize={grid.targetSize}
            sortField={grid.sortField}
            sortOrder={grid.sortOrder}
            onViewModeChange={gridActions.onViewModeChange}
            onSortFieldChange={gridActions.onSortFieldChange}
            onSortOrderChange={gridActions.onSortOrderChange}
            externalFreeze={grid.externalFreeze}
            viewer={viewer}
          />
          <MainViewProgressBar />
        </div>
      );
    case 'collections':
      return <div className={styles.frame}><Collections /></div>;
    case 'subscriptions':
      return (
        <div className={styles.frame}>
          <SubscriptionGroupsPanel
            onOpenCreateModal={subscriptions.onOpenCreateSubscriptionGroupModal}
            refreshToken={subscriptions.subscriptionRefreshToken}
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

export { CreateSubscriptionGroupModal };
