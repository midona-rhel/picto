import { useAtomValue, useSetAtom } from 'jotai';
import { IconAntennaBars5, IconPlus, IconShieldLock } from '@tabler/icons-react';
import type { SubscriptionProgressEvent } from '../../../shared/types/subscriptions';
import type { AuthSiteSnapshot, SubscriptionWorkspaceSnapshot } from '../../../shared/types/subscriptionsWorkspace';
import type { SubscriptionCreateFormState } from '../../../state/subscriptionsWorkspace';
import { subscriptionsWorkspaceTabAtom, setSubscriptionsWorkspaceTabAtom } from '../../../state/navigation';
import { WorkspaceSwitcher } from '../../../shared/ui/WorkspaceSwitcher';
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
  authSites,
  selectedAuthSiteId,
  onSelectAuthSite,
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
  authSites?: AuthSiteSnapshot[];
  selectedAuthSiteId?: string | null;
  onSelectAuthSite?: (siteId: string) => void;
}) {
  const workspaceTab = useAtomValue(subscriptionsWorkspaceTabAtom);
  const setWorkspaceTab = useSetAtom(setSubscriptionsWorkspaceTabAtom);
  const isAuth = workspaceTab === 'auth';

  return (
    <aside className={styles.sidebar}>
      <div className={styles.sourceToggle}>
        <WorkspaceSwitcher
          value={workspaceTab}
          onChange={setWorkspaceTab}
          options={[
            { value: 'subscriptions' as const, label: 'Subscriptions' },
            { value: 'auth' as const, label: 'Auth' },
          ]}
        />
      </div>

      {isAuth ? (
        <div className={styles.sidebarBody}>
          <div className={styles.list}>
            {(authSites ?? []).map((entry) => (
              <button
                key={entry.site.id}
                className={`${styles.subscriptionRow} ${selectedAuthSiteId === entry.site.id ? styles.subscriptionRowSelected : ''}`.trim()}
                onClick={() => onSelectAuthSite?.(entry.site.id)}
              >
                <div className={styles.sidebarItemIcon}>
                  <IconShieldLock size={16} />
                </div>
                <span className={styles.subName}>{entry.site.name}</span>
              </button>
            ))}
          </div>
        </div>
      ) : (
        <>
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
                  <div className={styles.sidebarItemIcon}>
                    <IconAntennaBars5 size={16} />
                  </div>
                  <span className={styles.subName}>{subscription.name}</span>
                  <span className={styles.sidebarItemCount}>{subscription.queries.length}</span>
                </button>
              ))}
            </div>

            {!snapshot?.subscriptions.length && !showCreateForm && (
              <EmptyState title="No subscriptions yet" description="Create a subscription container, then add site queries." />
            )}
          </div>
        </>
      )}
    </aside>
  );
}
