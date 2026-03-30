import { IconPlus } from '@tabler/icons-react';
import type { SubscriptionProgressEvent } from '../../../shared/types/subscriptions';
import type { SubscriptionWorkspaceSnapshot } from '../../../controllers/subscriptionsController';
import type { SubscriptionCreateFormState } from '../../../state/subscriptionsWorkspace';
import { ActionButton } from './ActionButton';
import { EmptyState } from './EmptyState';
import styles from '../SubscriptionsScreen.module.css';

export function SubscriptionsSidebar({
  snapshot,
  error,
  selectedSubscriptionId,
  progressBySubscriptionId: _progressBySubscriptionId,
  showCreateForm,
  createForm,
  createBusy,
  onToggleCreateForm,
  onSelectSubscription,
  onCreateFormChange,
  onCreate,
  onCancelCreate,
}: {
  snapshot: SubscriptionWorkspaceSnapshot | null;
  error: string | null;
  selectedSubscriptionId: string | null;
  progressBySubscriptionId: Map<string, SubscriptionProgressEvent>;
  showCreateForm: boolean;
  createForm: SubscriptionCreateFormState;
  createBusy: boolean;
  onToggleCreateForm: () => void;
  onSelectSubscription: (id: string) => void;
  onCreateFormChange: (patch: Partial<SubscriptionCreateFormState>) => void;
  onCreate: () => Promise<void>;
  onCancelCreate: () => void;
}) {
  return (
    <aside className={styles.sidebar}>
      <div className={styles.sidebarHeader}>
        <span className={styles.sidebarLabel}>Subscriptions</span>
        <ActionButton variant="primary" compact onClick={onToggleCreateForm}>
          <IconPlus size={14} />
        </ActionButton>
      </div>

      <div className={styles.sidebarBody}>
        {showCreateForm && (
          <div className={styles.createCard}>
            <div className={styles.sectionHeader}>
              <div className={styles.sectionTitle}>New Subscription</div>
            </div>
            <label className={styles.label}>
              Name
              <input className={styles.field} value={createForm.name} onChange={(e) => onCreateFormChange({ name: e.target.value })} />
            </label>
            <div className={styles.gridTwo}>
              <label className={styles.label}>
                Initial Post Limit
                <input className={styles.field} value={createForm.initialPostLimit} onChange={(e) => onCreateFormChange({ initialPostLimit: e.target.value })} />
              </label>
              <label className={styles.label}>
                Periodic Post Limit
                <input className={styles.field} value={createForm.periodicPostLimit} onChange={(e) => onCreateFormChange({ periodicPostLimit: e.target.value })} />
              </label>
            </div>
            <div className={styles.inlineActions}>
              <ActionButton variant="primary" compact disabled={createBusy || !createForm.name.trim()} onClick={() => { void onCreate(); }}>
                Create
              </ActionButton>
              <ActionButton variant="ghost" compact disabled={createBusy} onClick={onCancelCreate}>
                Cancel
              </ActionButton>
            </div>
          </div>
        )}

        {error && <div className={styles.errorBanner}>{error}</div>}

        <div className={styles.list}>
          {(snapshot?.subscriptions ?? []).map((subscription) => (
            <button
              key={subscription.id}
              className={`${styles.subscriptionRow} ${selectedSubscriptionId === subscription.id ? styles.subscriptionRowSelected : ''}`.trim()}
              onClick={() => onSelectSubscription(subscription.id)}
            >
              <span className={styles.subName}>{subscription.name}</span>
            </button>
          ))}
        </div>

        {!snapshot?.subscriptions.length && !showCreateForm && (
          <EmptyState title="No subscriptions yet" description="Create a subscription container from the left rail, then add one or more site queries." />
        )}
      </div>
    </aside>
  );
}
