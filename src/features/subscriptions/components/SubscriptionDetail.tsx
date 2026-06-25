import { useEffect, useRef, useState } from 'react';
import {
  IconDotsVertical,
  IconPlayerPause,
  IconPlayerPlay,
  IconPlayerStop,
} from '@tabler/icons-react';
import type {
  SubscriptionGroupInfo,
  SubscriptionInfo,
  SubscriptionProgressEvent,
  SubscriptionQueryInfo,
} from '../../../shared/types/subscriptions';
import type { FailedPostGroup } from '../../../shared/types/subscriptions';
import type { SubscriptionDetailState, SubscriptionDetailTab } from '../../../state/subscriptionsWorkspace';
import type { SubscriptionWorkspaceSnapshot } from '../../../shared/types/subscriptionsWorkspace';
import { CmSelect } from '../../../shared/ui/CmSelect/CmSelect';
import { ToggleSwitch } from '../../../shared/ui/ToggleSwitch/ToggleSwitch';
import { ActionButton } from './ActionButton';
import { AddQueryBar } from './AddQueryBar';
import { EmptyState } from './EmptyState';
import { HealthTab } from './HealthTab';
import { HistoryTab } from './HistoryTab';
import { LiveProgressCard } from './LiveProgressCard';
import { QueryEditModal } from './QueryEditModal';
import { QueryRow } from './QueryRow';
import { StatusBadge } from './StatusBadge';
import {
  describeSubscriptionState,
  getQueryAuthState,
  getQueryFailedCount,
} from '../subscriptionUtils';
import styles from '../SubscriptionsScreen.module.css';

/** Above this pane width, tabs disappear and Health/History become a side column. */
const WIDE_BREAKPOINT_PX = 1080;

const TABS: Array<{ key: SubscriptionDetailTab; label: string }> = [
  { key: 'queries', label: 'Queries' },
  { key: 'health', label: 'Health' },
  { key: 'history', label: 'History' },
];

export interface DetailController {
  run: (id: string) => void;
  stop: (id: string) => void;
  pause: (id: string, paused: boolean) => void;
  reset: (id: string) => void;
  delete: (id: string) => void;
  setAutoCollections: (id: string, on: boolean) => void;
  setGroup: (id: string, groupId: number | null) => void;
  runQuery: (subscriptionId: string, queryId: string) => void;
  stopQuery: (subscriptionId: string, queryId: string) => void;
  pauseQuery: (queryId: string, paused: boolean) => void;
  deleteQuery: (queryId: string) => void;
  editQuery: (queryId: number, siteId: string, queryText: string, displayName: string | null, notes: string | null) => Promise<void>;
  addQuery: (subscriptionId: string, siteId: string, queryText: string) => Promise<void>;
  retryFailedPosts: (posts: FailedPostGroup[]) => void;
  openExternalUrl: (url: string) => void;
}

function OverflowMenu({
  subscription,
  groups,
  busy,
  running,
  controller,
}: {
  subscription: SubscriptionInfo;
  groups: SubscriptionGroupInfo[];
  busy: boolean;
  running: boolean;
  controller: DetailController;
}) {
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (!wrapRef.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener('mousedown', handler);
    return () => window.removeEventListener('mousedown', handler);
  }, [open]);

  return (
    <div className={styles.menuWrap} ref={wrapRef}>
      <button
        type="button"
        className={styles.querySmallBtn}
        aria-label="More actions"
        onClick={() => setOpen((v) => !v)}
      >
        <IconDotsVertical size={16} />
      </button>
      {open && (
        <div className={styles.menuPopover}>
          <div className={styles.menuRow}>
            <span className={styles.menuRowLabel}>Group</span>
            <CmSelect
              value={subscription.group_id ?? ''}
              options={[
                { value: '', label: 'No group' },
                ...groups.map((group) => ({ value: group.id, label: group.name })),
              ]}
              onChange={(value) => {
                controller.setGroup(subscription.id, value ? Number.parseInt(value, 10) : null);
                setOpen(false);
              }}
              width={150}
            />
          </div>
          <div
            className={styles.menuRow}
            role="menuitemcheckbox"
            aria-checked={subscription.auto_collections}
          >
            <span className={styles.menuRowLabel}>
              Combine multi-image posts
              <span className={styles.helper}>into one collection</span>
            </span>
            <ToggleSwitch
              on={subscription.auto_collections}
              onChange={() => controller.setAutoCollections(subscription.id, !subscription.auto_collections)}
            />
          </div>
          <div className={styles.menuDivider} />
          <button
            type="button"
            className={styles.menuItem}
            disabled={busy || running}
            onClick={() => {
              setOpen(false);
              controller.reset(subscription.id);
            }}
          >
            Reset sync progress…
          </button>
          <button
            type="button"
            className={`${styles.menuItem} ${styles.menuItemDanger}`}
            disabled={busy || running}
            onClick={() => {
              setOpen(false);
              controller.delete(subscription.id);
            }}
          >
            Delete subscription…
          </button>
        </div>
      )}
    </div>
  );
}

export function SubscriptionDetail({
  subscription,
  snapshot,
  groups,
  progress,
  detail,
  activeTab,
  busy,
  controller,
  onTabChange,
  onOpenAccounts,
}: {
  subscription: SubscriptionInfo;
  snapshot: SubscriptionWorkspaceSnapshot;
  groups: SubscriptionGroupInfo[];
  progress: SubscriptionProgressEvent | null;
  detail: SubscriptionDetailState;
  activeTab: SubscriptionDetailTab;
  busy: boolean;
  controller: DetailController;
  onTabChange: (tab: SubscriptionDetailTab) => void;
  onOpenAccounts: (siteId: string | null) => void;
}) {
  const [editing, setEditing] = useState<SubscriptionQueryInfo | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const [wide, setWide] = useState(false);

  useEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    const observer = new ResizeObserver(() => {
      setWide(el.clientWidth >= WIDE_BREAKPOINT_PX);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const metrics = snapshot.listMetrics[subscription.id];
  const running = progress != null;
  const state = describeSubscriptionState({
    paused: subscription.paused,
    progress,
    failedPostCount: metrics?.failedPostCount ?? 0,
    openIssueCount: metrics?.openIssueCount ?? 0,
  });
  const failedBadge = (metrics?.failedPostCount ?? 0) + (metrics?.openIssueCount ?? 0);
  const groupName = groups.find((group) => group.id === subscription.group_id)?.name ?? null;

  const queriesSection = (
    <div className={styles.tabPanel}>
      {subscription.queries.length === 0 ? (
        <EmptyState
          title="Nothing followed yet"
          description="Add a query below — a tag search or an account to follow on a site."
        />
      ) : (
        subscription.queries.map((query) => {
          const auth = getQueryAuthState({
            query,
            sites: snapshot.sites,
            credentials: snapshot.credentials,
            credentialHealth: snapshot.credentialHealth,
          });
          const queryRunning = running && progress?.query_id === query.id;
          return (
            <QueryRow
              key={query.id}
              query={query}
              sites={snapshot.sites}
              running={queryRunning || (running && progress?.query_id == null)}
              paused={query.paused}
              failedCount={getQueryFailedCount(query.id, detail.failedPosts)}
              authWarning={auth.tone === 'attention' ? auth.label : null}
              progress={queryRunning ? progress : null}
              busy={busy}
              onRun={() => controller.runQuery(subscription.id, query.id)}
              onStop={() => controller.stopQuery(subscription.id, query.id)}
              onPause={(paused) => controller.pauseQuery(query.id, paused)}
              onEdit={() => setEditing(query)}
              onDelete={() => controller.deleteQuery(query.id)}
              onOpenAuth={() => onOpenAccounts(query.site_id)}
            />
          );
        })
      )}
      <AddQueryBar
        sites={snapshot.sites}
        busy={busy}
        onAdd={(siteId, queryText) => controller.addQuery(subscription.id, siteId, queryText)}
      />
    </div>
  );

  const healthSection = (
    <HealthTab
      failedPosts={detail.failedPosts}
      issues={detail.issues}
      busy={busy}
      onRetryPosts={controller.retryFailedPosts}
      onOpenUrl={controller.openExternalUrl}
    />
  );

  const historySection = <HistoryTab runs={detail.runs} />;

  return (
    <div className={styles.content} ref={rootRef}>
      <div className={styles.hero}>
        <div className={styles.heroTop}>
          <div className={styles.titleWrap}>
            <span className={styles.heroTitle}>{subscription.name}</span>
            <span className={styles.heroMeta}>
              <StatusBadge
                tone={state}
                label={state === 'running' ? 'Running' : state === 'paused' ? 'Paused' : state === 'attention' ? 'Needs attention' : 'Idle'}
              />
              <span className={styles.muted}>{subscription.total_files.toLocaleString()} files</span>
              {groupName && <span className={styles.muted}>in {groupName}</span>}
            </span>
          </div>
          <div className={styles.heroActions}>
            {running ? (
              <ActionButton variant="secondary" disabled={busy} onClick={() => controller.stop(subscription.id)}>
                <IconPlayerStop size={14} /> Stop
              </ActionButton>
            ) : (
              <ActionButton
                variant="primary"
                disabled={busy || subscription.paused || subscription.queries.length === 0}
                onClick={() => controller.run(subscription.id)}
              >
                <IconPlayerPlay size={14} /> Run now
              </ActionButton>
            )}
            <ActionButton
              variant="secondary"
              disabled={busy || running}
              onClick={() => controller.pause(subscription.id, !subscription.paused)}
            >
              <IconPlayerPause size={14} /> {subscription.paused ? 'Resume' : 'Pause'}
            </ActionButton>
            <OverflowMenu
              subscription={subscription}
              groups={groups}
              busy={busy}
              running={running}
              controller={controller}
            />
          </div>
        </div>
      </div>

      {progress && <LiveProgressCard progress={progress} />}

      {wide ? (
        <div className={styles.detailColumns}>
          <div className={styles.mainColumn}>
            <div className={styles.sectionHeader}>
              <span className={styles.sectionTitle}>Queries</span>
            </div>
            {queriesSection}
          </div>
          <div className={styles.sideColumn}>
            <div className={styles.sectionHeader}>
              <span className={styles.sectionTitle}>Health</span>
              {failedBadge > 0 && (
                <span className={`${styles.railBadge} ${styles.railBadgeAttention}`}>{failedBadge}</span>
              )}
            </div>
            {healthSection}
            <div className={styles.sectionHeader}>
              <span className={styles.sectionTitle}>History</span>
            </div>
            {historySection}
          </div>
        </div>
      ) : (
        <>
          <div className={styles.tabList}>
            {TABS.map((tab) => (
              <button
                key={tab.key}
                type="button"
                className={`${styles.tabButton} ${activeTab === tab.key ? styles.tabButtonActive : ''}`.trim()}
                onClick={() => onTabChange(tab.key)}
              >
                {tab.label}
                {tab.key === 'health' && failedBadge > 0 && (
                  <span className={`${styles.railBadge} ${styles.railBadgeAttention}`}>{failedBadge}</span>
                )}
              </button>
            ))}
          </div>
          {activeTab === 'queries' && queriesSection}
          {activeTab === 'health' && healthSection}
          {activeTab === 'history' && historySection}
        </>
      )}

      <QueryEditModal
        query={editing}
        sites={snapshot.sites}
        busy={busy}
        onClose={() => setEditing(null)}
        onSave={async (input) => {
          if (!editing) return;
          await controller.editQuery(
            Number.parseInt(editing.id, 10),
            input.siteId,
            input.queryText,
            input.displayName,
            input.notes,
          );
          setEditing(null);
        }}
      />
    </div>
  );
}
