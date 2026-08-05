import { useRef, useState } from 'react';
import { useAtom } from 'jotai';
import {
  IconAlertTriangle,
  IconCircleCheck,
  IconDotsVertical,
  IconPlayerPause,
  IconPlayerPlay,
  IconPlayerStop,
  IconRefresh,
  IconShieldLock,
} from '@tabler/icons-react';
import type {
  SubscriptionGroupInfo,
  SubscriptionInfo,
  SubscriptionProgressEvent,
  SubscriptionQueryInfo,
} from '../../../shared/types/subscriptions';
import type { FailedPostGroup } from '../../../shared/types/subscriptions';
import {
  subscriptionsDetailModeAtom,
  type SubscriptionDetailState,
  type SubscriptionDetailTab,
} from '../../../state/subscriptionsWorkspace';
import type { SubscriptionWorkspaceSnapshot } from '../../../shared/types/subscriptionsWorkspace';
import { KbdTooltip } from '../../../shared/ui/KbdTooltip';
import { CmSelect } from '../../../shared/ui/CmSelect/CmSelect';
import { ActionButton } from './ActionButton';
import { AddQueryBar } from './AddQueryBar';
import { HealthTab } from './HealthTab';
import { HistoryTab } from './HistoryTab';
import { LiveProgressCard } from './LiveProgressCard';
import { QueryEditModal } from './QueryEditModal';
import { QueryRow } from './QueryRow';
import { StatusBadge } from './StatusBadge';
import { mediaThumbnailUrl } from '../../../shared/lib/mediaUrl';
import {
  describeSubscriptionState,
  formatRelativeTime,
  getQueryAuthState,
  getQueryFailedCount,
  getSiteLabel,
  isQueryUpToDate,
  isSubscriptionUpToDate,
} from '../subscriptionUtils';
import styles from '../SubscriptionsScreen.module.css';

export interface DetailController {
  run: (id: string) => void;
  stop: (id: string) => void;
  pause: (id: string, paused: boolean) => void;
  reset: (id: string) => void;
  delete: (id: string) => void;
  rename: (id: string, currentName: string) => void;
  setAutoCollections: (id: string, on: boolean) => void;
  setSchedule: (id: string, schedule: string) => void;
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

const SCHEDULE_OPTIONS = [
  { value: 'manual', label: 'Manual' },
  { value: 'daily', label: 'Daily' },
  { value: 'weekly', label: 'Weekly' },
  { value: 'monthly', label: 'Monthly' },
];

/** "⋮" trigger for the shared subscription context menu, anchored below the button. */
function OverflowMenuButton({ onOpen }: { onOpen: (position: { x: number; y: number }) => void }) {
  const buttonRef = useRef<HTMLButtonElement>(null);
  return (
    <KbdTooltip label="More actions">
      <button
        type="button"
        ref={buttonRef}
        className={styles.querySmallBtn}
        aria-label="More actions"
        onClick={() => {
          const rect = buttonRef.current?.getBoundingClientRect();
          if (rect) onOpen({ x: rect.left, y: rect.bottom + 4 });
        }}
      >
        <IconDotsVertical size={16} />
      </button>
    </KbdTooltip>
  );
}

/**
 * Single-column detail pane: header, live progress while running, then
 * Queries / Health / History as stacked dense sections. Health collapses
 * to a one-line "all healthy" note when there is nothing to fix.
 */
export function SubscriptionDetail({
  subscription,
  snapshot,
  groups,
  progress,
  detail,
  coverHash = null,
  busy,
  controller,
  onOpenAccounts,
  onOpenMenu,
}: {
  subscription: SubscriptionInfo;
  snapshot: SubscriptionWorkspaceSnapshot;
  groups: SubscriptionGroupInfo[];
  progress: SubscriptionProgressEvent | null;
  detail: SubscriptionDetailState;
  /** Newest downloaded file — hero image; null falls back to an initial. */
  coverHash?: string | null;
  /** Legacy tab props kept out — layout is single-column now. */
  activeTab?: SubscriptionDetailTab;
  busy: boolean;
  controller: DetailController;
  onTabChange?: (tab: SubscriptionDetailTab) => void;
  onOpenAccounts: (siteId: string | null) => void;
  /** Opens the shared subscription context menu at a screen position. */
  onOpenMenu: (position: { x: number; y: number }) => void;
}) {
  const [editing, setEditing] = useState<SubscriptionQueryInfo | null>(null);
  const [mode, setMode] = useAtom(subscriptionsDetailModeAtom);

  const metrics = snapshot.listMetrics[subscription.id];
  const running = progress != null;
  const state = describeSubscriptionState({
    paused: subscription.paused,
    progress,
    failedPostCount: metrics?.failedPostCount ?? 0,
    openIssueCount: metrics?.openIssueCount ?? 0,
  });
  const groupName = groups.find((group) => group.id === subscription.group_id)?.name ?? null;
  const openIssueCount = detail.issues.filter((issue) => issue.status !== 'resolved').length;
  const healthy = detail.failedPosts.length === 0 && openIssueCount === 0;
  const upToDate = !running && isSubscriptionUpToDate(
    subscription,
    detail.failedPosts.length,
    openIssueCount,
  );

  // Plain-language facts for the overview
  const lastCheckTime = subscription.queries.reduce<string | null>(
    (latest, query) =>
      query.last_check_time && (!latest || query.last_check_time > latest) ? query.last_check_time : latest,
    null,
  );
  const authAttention = subscription.queries
    .map((query) => ({
      query,
      auth: getQueryAuthState({
        query,
        sites: snapshot.sites,
        credentials: snapshot.credentials,
        credentialHealth: snapshot.credentialHealth,
      }),
    }))
    .find((entry) => entry.auth.tone === 'attention');
  const retryable = detail.failedPosts.filter((post) => post.canRetry);
  const failedCount = detail.failedPosts.length;
  const lastRun = detail.runs[0] ?? null;

  const overviewSection = (
    <div className={styles.overview}>
      {running && progress ? (
        <div className={styles.ovStatus}>
          <span className={`${styles.ovIcon} ${styles.ovIconRunning}`.trim()}>
            <IconRefresh size={18} />
          </span>
          <div className={styles.ovStatusText}>
            <span className={styles.ovHeadline}>Checking for new posts…</span>
            <span className={styles.ovSub}>
              {progress.files_downloaded} downloaded · {progress.ingested} added to your library so far
            </span>
          </div>
        </div>
      ) : subscription.paused ? (
        <div className={styles.ovStatus}>
          <span className={styles.ovIcon}><IconPlayerPause size={18} /></span>
          <div className={styles.ovStatusText}>
            <span className={styles.ovHeadline}>Paused</span>
            <span className={styles.ovSub}>Not checking for new posts. Press Resume to pick up where it left off.</span>
          </div>
        </div>
      ) : !healthy || authAttention ? (
        <div className={styles.ovStatus}>
          <span className={`${styles.ovIcon} ${styles.ovIconWarn}`.trim()}>
            <IconAlertTriangle size={18} />
          </span>
          <div className={styles.ovStatusText}>
            <span className={styles.ovHeadline}>Needs a look</span>
            <span className={styles.ovSub}>
              {failedCount > 0 &&
                `${failedCount} post${failedCount === 1 ? '' : 's'} couldn’t be downloaded. `}
              {authAttention &&
                `Your ${getSiteLabel(authAttention.query.site_id, snapshot.sites)} login needs attention.`}
              {failedCount === 0 && !authAttention && 'Something needs attention — see the technical view.'}
            </span>
            <span className={styles.ovActions}>
              {retryable.length > 0 && (
                <ActionButton
                  variant="secondary"
                  compact
                  disabled={busy}
                  onClick={() => controller.retryFailedPosts(retryable)}
                >
                  <IconRefresh size={13} /> Try again
                </ActionButton>
              )}
              {authAttention && (
                <ActionButton
                  variant="secondary"
                  compact
                  onClick={() => onOpenAccounts(authAttention.query.site_id)}
                >
                  <IconShieldLock size={13} /> Fix login…
                </ActionButton>
              )}
            </span>
          </div>
        </div>
      ) : subscription.total_files === 0 &&
        subscription.queries.some((query) => !query.completed_initial_run) ? (
        <div className={styles.ovStatus}>
          <span className={styles.ovIcon}><IconPlayerPlay size={18} /></span>
          <div className={styles.ovStatusText}>
            <span className={styles.ovHeadline}>Ready for its first sync</span>
            <span className={styles.ovSub}>
              Nothing has been downloaded yet. Press Run now to fetch the first posts.
            </span>
          </div>
        </div>
      ) : (
        <div className={styles.ovStatus}>
          <span className={`${styles.ovIcon} ${styles.ovIconOk}`.trim()}>
            <IconCircleCheck size={18} />
          </span>
          <div className={styles.ovStatusText}>
            <span className={styles.ovHeadline}>Everything is fine</span>
            <span className={styles.ovSub}>
              {lastCheckTime ? `Last checked ${formatRelativeTime(lastCheckTime)}` : 'Not checked yet'}
              {' · '}
              {subscription.total_files.toLocaleString()} files collected
              {lastRun && lastRun.files_downloaded > 0 &&
                ` · last run fetched ${lastRun.files_downloaded}`}
              {lastRun && lastRun.files_skipped > 0 &&
                ` (${lastRun.files_skipped} duplicate reused)`}
            </span>
          </div>
        </div>
      )}

      <div className={styles.ovQueries}>
        <span className={styles.subsectionTitle}>Queries</span>
        {subscription.queries.length === 0 ? (
          <div className={styles.sectionEmptyLine}>
            Nothing yet — switch to Technical to add a tag search or an account.
          </div>
        ) : (
          subscription.queries.map((query) => (
            <div key={query.id} className={styles.ovFollowRow}>
              <span className={styles.ovFollowName}>
                {query.display_name?.trim() || query.query_text}
                {!running && isQueryUpToDate(query, getQueryFailedCount(query.id, detail.failedPosts)) && (
                  <span className={styles.upToDateChip}>Up to date</span>
                )}
              </span>
              <span className={styles.ovFollowMeta}>
                on {getSiteLabel(query.site_id, snapshot.sites)}
                {' · '}
                {query.files_found.toLocaleString()} files
                {query.paused && ' · paused'}
                {!query.completed_initial_run && ' · first sync still running'}
              </span>
            </div>
          ))
        )}
      </div>
    </div>
  );

  return (
    <div className={styles.content}>
      <div className={styles.hero}>
        <div className={styles.heroTop}>
          <div className={styles.heroIdentity}>
            <span className={styles.heroCover}>
              {coverHash ? (
                <img src={mediaThumbnailUrl(coverHash)} alt="" draggable={false} />
              ) : (
                <span className={styles.heroCoverFallback} aria-hidden>
                  {subscription.name.slice(0, 1).toUpperCase()}
                </span>
              )}
            </span>
          <div className={styles.titleWrap}>
            <KbdTooltip label="Double-click to rename">
              <span
                className={styles.heroTitle}
                onDoubleClick={() => controller.rename(subscription.id, subscription.name)}
              >
                {subscription.name}
              </span>
            </KbdTooltip>
            <span className={styles.heroMeta}>
              <StatusBadge
                tone={upToDate ? 'success' : state}
                label={upToDate ? 'Up to date' : state === 'running' ? 'Running' : state === 'paused' ? 'Paused' : state === 'attention' ? 'Needs attention' : 'Idle'}
              />
              <span className={styles.muted}>{subscription.total_files.toLocaleString()} files</span>
              {groupName && <span className={styles.muted}>in {groupName}</span>}
            </span>
          </div>
          </div>
          <div className={styles.heroActions}>
            <span className={styles.fieldInline}>
              Schedule
              <CmSelect
                value={subscription.schedule}
                options={SCHEDULE_OPTIONS}
                onChange={(schedule) => controller.setSchedule(subscription.id, schedule)}
                width={100}
              />
            </span>
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
            <OverflowMenuButton onOpen={onOpenMenu} />
          </div>
        </div>
      </div>

      {progress && mode === 'technical' && <LiveProgressCard progress={progress} />}

      <div className={styles.modeToggleRow}>
        <div className={styles.modeToggle} role="tablist" aria-label="Detail level">
          <button
            type="button"
            role="tab"
            aria-selected={mode === 'overview'}
            className={`${styles.modeToggleBtn} ${mode === 'overview' ? styles.modeToggleBtnActive : ''}`.trim()}
            onClick={() => setMode('overview')}
          >
            Overview
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={mode === 'technical'}
            className={`${styles.modeToggleBtn} ${mode === 'technical' ? styles.modeToggleBtnActive : ''}`.trim()}
            onClick={() => setMode('technical')}
          >
            Technical
          </button>
        </div>
      </div>

      {mode === 'overview' ? overviewSection : (
      <div className={styles.detailSections}>
        <section className={styles.detailSection}>
          <div className={styles.sectionHeader}>
            <span className={styles.sectionTitle}>Queries</span>
          </div>
          {subscription.queries.length > 0 && (
            <div className={styles.qTable}>
              <div className={`${styles.qRow} ${styles.qHeader}`}>
                <span>Query</span>
                <span>Site</span>
                <span className={styles.qCellNum}>Posts</span>
                <span className={styles.qCellNum}>Files</span>
                <span>Last check</span>
                <span>Status</span>
                <span />
              </div>
              {subscription.queries.map((query) => {
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
              })}
            </div>
          )}
          {subscription.queries.length === 0 && (
            <div className={styles.sectionEmptyLine}>
              Nothing followed yet — add a tag search or an account below.
            </div>
          )}
          <AddQueryBar
            sites={snapshot.sites}
            busy={busy}
            onAdd={(siteId, queryText) => controller.addQuery(subscription.id, siteId, queryText)}
          />
        </section>

        <section className={styles.detailSection}>
          <div className={styles.sectionHeader}>
            <span className={styles.sectionTitle}>Health</span>
          </div>
          {detail.loading ? (
            <div className={styles.sectionEmptyLine}>Checking…</div>
          ) : healthy ? (
            <div className={styles.healthyLine}>
              <IconCircleCheck size={14} /> All healthy — no failed posts, no open issues.
            </div>
          ) : (
            <HealthTab
              failedPosts={detail.failedPosts}
              issues={detail.issues}
              busy={busy}
              onRetryPosts={controller.retryFailedPosts}
              onOpenUrl={controller.openExternalUrl}
            />
          )}
        </section>

        <section className={styles.detailSection}>
          <div className={styles.sectionHeader}>
            <span className={styles.sectionTitle}>History</span>
          </div>
          {detail.loading ? (
            <div className={styles.sectionEmptyLine}>Loading…</div>
          ) : (
            <HistoryTab runs={detail.runs} />
          )}
        </section>
      </div>
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
