import { useMemo, useRef, useState } from 'react';
import { useAtomValue, useSetAtom } from 'jotai';
import { IconDotsVertical, IconDownload, IconPlayerPause, IconPlayerPlay, IconPlayerStop, IconPlus } from '@tabler/icons-react';
import type { SubscriptionCover, SubscriptionInfo, SubscriptionProgressEvent, SubscriptionQueryInfo } from '../../../shared/types/subscriptions';
import type { SubscriptionDetailState } from '../../../state/subscriptionsWorkspace';
import type { SubscriptionWorkspaceSnapshot } from '../../../shared/types/subscriptionsWorkspace';
import { KbdTooltip } from '../../../shared/ui/KbdTooltip';
import { CmSelect } from '../../../shared/ui/CmSelect/CmSelect';
import { folderNodesAtom } from '../../../state/sidebar';
import { folderPickerPortalAtom, tagSelectPortalAtom } from '../../../state/portals';
import { TagChip } from '../../../shared/ui/TagChip/TagChip';
import { TagAssignmentControl } from '../../../shared/ui/TagAssignmentControl';
import { GlassModal } from '../../../shared/ui/GlassModal/GlassModal';
import { CompactNumberInput } from '../../../shared/ui/CompactNumberInput/CompactNumberInput';
import { ActionButton } from './ActionButton';
import { AddQueryBar } from './AddQueryBar';
import { HealthTab } from './HealthTab';
import { HistoryTab } from './HistoryTab';
import { QueryEditModal } from './QueryEditModal';
import { QueryRow } from './QueryRow';
import { SubscriptionCoverDisplay } from './SubscriptionCoverImage';
import { StatusBadge } from './StatusBadge';
import {
  describeSubscriptionState,
  describeFailure,
  formatRelativeTime,
  getQueryAuthState,
  getSiteLabel,
  getSubscriptionRunTarget,
  isSubscriptionCompleted,
} from '../subscriptionUtils';
import styles from '../SubscriptionsScreen.module.css';

export interface DetailController {
  run: (id: string) => void;
  stop: (id: string) => void;
  pause: (id: string, paused: boolean) => void;
  delete: (id: string) => void;
  rename: (id: string, currentName: string) => void;
  setSchedule: (id: string, schedule: string) => void;
  setPostsPerRun: (id: string, postsPerRun: number) => void;
  setDestination: (id: string, destination: { target_folder_ids: number[]; automatic_tags: string[] }) => Promise<void>;
  pauseQuery: (queryId: string, paused: boolean) => void;
  setQueryGrouping: (queryId: string, groupPosts: boolean) => void;
  deleteQuery: (queryId: string) => void;
  editQuery: (queryId: number, siteId: string, queryText: string, displayName: string | null, notes: string | null) => Promise<void>;
  addQuery: (subscriptionId: string, siteId: string, queryText: string) => Promise<void>;
  openExternalUrl: (url: string) => void;
}

type DetailTab = 'sources' | 'history' | 'problems';

const SCHEDULE_OPTIONS = [
  { value: 'manual', label: 'Manual' },
  { value: 'daily', label: 'Daily' },
  { value: 'weekly', label: 'Weekly' },
  { value: 'monthly', label: 'Monthly' },
];

function OverflowMenuButton({ onOpen }: { onOpen: (position: { x: number; y: number }) => void }) {
  const buttonRef = useRef<HTMLButtonElement>(null);
  return (
    <KbdTooltip label="More actions">
      <button
        type="button"
        ref={buttonRef}
        className={`${styles.querySmallBtn} ${styles.subscriptionOverflowButton}`.trim()}
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

function PostsPerRunInput({
  value,
  disabled,
  onCommit,
}: {
  value: number;
  disabled: boolean;
  onCommit: (value: number) => void;
}) {
  return (
    <CompactNumberInput value={value} min={1} max={10_000} label="Posts per run"
      disabled={disabled} onCommit={onCommit} />
  );
}

function hexToRgb(hex: string | null): [number, number, number] | undefined {
  if (!hex) return undefined;
  const normalized = hex.replace('#', '');
  if (!/^[0-9a-f]{6}$/i.test(normalized)) return undefined;
  return [0, 2, 4].map((offset) => Number.parseInt(normalized.slice(offset, offset + 2), 16)) as [number, number, number];
}

/** A fixed desktop workspace: persistent identity rail and one tabbed data plane. */
export function SubscriptionDetail({
  subscription,
  snapshot,
  progress,
  detail,
  cover = null,
  busy,
  controller,
  onOpenAccounts,
  onLoadMoreHealth,
  onOpenMenu,
}: {
  subscription: SubscriptionInfo;
  snapshot: SubscriptionWorkspaceSnapshot;
  progress: SubscriptionProgressEvent | null;
  detail: SubscriptionDetailState;
  cover?: SubscriptionCover | null;
  busy: boolean;
  controller: DetailController;
  onOpenAccounts: (siteId: string | null) => void;
  onLoadMoreHealth: () => void;
  onOpenMenu: (position: { x: number; y: number }) => void;
}) {
  const [editing, setEditing] = useState<SubscriptionQueryInfo | null>(null);
  const [statsQuery, setStatsQuery] = useState<SubscriptionQueryInfo | null>(null);
  const [tab, setTab] = useState<DetailTab>('sources');
  const folders = useAtomValue(folderNodesAtom);
  const openTagPicker = useSetAtom(tagSelectPortalAtom);
  const openFolderPicker = useSetAtom(folderPickerPortalAtom);

  const metrics = snapshot.listMetrics[subscription.id];
  const waitingForInbox = subscription.run_status === 'inbox_full';
  const running = progress != null || snapshot.runningSubscriptionIds.includes(subscription.id);
  const state = waitingForInbox || subscription.paused ? 'paused' : running ? 'running' : describeSubscriptionState({
    paused: subscription.paused,
    progress,
    failedPostCount: metrics?.failedPostCount ?? 0,
    openIssueCount: metrics?.openIssueCount ?? 0,
  });
  const problemCount = detail.issueTotalCount + detail.failedPostTotalCount;
  const completed = !running && isSubscriptionCompleted(subscription, detail.failedPostTotalCount, detail.issueTotalCount);
  const lastCheck = useMemo(() => subscription.queries.reduce<string | null>(
    (latest, query) => query.last_check_time && (!latest || query.last_check_time > latest) ? query.last_check_time : latest,
    null,
  ), [subscription.queries]);
  const latestRun = detail.runs[0] ?? null;
  const persistedPostCount = subscription.queries.reduce((total, query) => total + query.posts_found, 0);
  const traversedCount = progress?.posts_traversed ?? latestRun?.posts_traversed ?? 0;
  const postsAddedCount = progress?.posts_added ?? latestRun?.posts_added ?? persistedPostCount;
  const runTarget = getSubscriptionRunTarget(subscription);
  const runTraversed = Math.min(progress?.posts_traversed ?? 0, runTarget);
  const runProgressPercent = runTarget > 0 ? Math.min(100, (runTraversed / runTarget) * 100) : 0;
  const failedQuery = useMemo(() => subscription.queries.reduce<SubscriptionQueryInfo | null>((latest, query) => {
    if (!query.last_failure_message) return latest;
    if (!latest?.last_failure_at) return query;
    return query.last_failure_at && query.last_failure_at > latest.last_failure_at ? query : latest;
  }, null), [subscription.queries]);
  const concreteProblem = detail.issues.find((issue) => issue.status !== 'resolved')?.message
    ?? describeFailure(failedQuery?.last_failure_kind ?? null, failedQuery?.last_failure_message ?? null)
    ?? (detail.failedPostTotalCount > 0 ? `${detail.failedPostTotalCount} downloads failed` : null);
  const statusLabel = waitingForInbox
    ? 'Inbox full'
    : completed
      ? 'Complete'
    : state === 'running'
      ? 'Syncing'
      : state === 'paused'
        ? 'Paused'
        : state === 'attention'
          ? 'Needs attention'
          : 'Idle';
  const statusTone = state === 'running'
    ? 'running' as const
    : state === 'paused'
      ? 'paused' as const
      : state === 'attention'
        ? 'attention' as const
        : completed
          ? 'success' as const
          : 'idle' as const;

  const selectedFolders = useMemo(() => {
    const selected = new Set(subscription.target_folder_ids);
    return folders.filter((folder) => selected.has(Number(folder.id.slice('folder:'.length))));
  }, [folders, subscription.target_folder_ids]);

  const tabs: Array<{ id: DetailTab; label: string }> = [
    { id: 'sources', label: 'Sources' },
    { id: 'history', label: 'History' },
    { id: 'problems', label: 'Problems' },
  ];

  return (
    <div className={styles.subscriptionWorkspace}>
      <aside className={styles.subscriptionRail}>
        <div className={styles.subscriptionIdentity}>
          <span className={styles.subscriptionCover}>
            {cover ? (
              <SubscriptionCoverDisplay
                fileHash={cover.file_hash}
                crop={{
                  focusX: cover.focus_x,
                  focusY: cover.focus_y,
                  zoomPercent: cover.zoom_percent,
                }}
                alt=""
                draggable={false}
              />
            ) : (
              <span className={styles.subscriptionCoverFallback} aria-hidden>
                <IconDownload size={30} stroke={1.35} />
              </span>
            )}
          </span>
          <KbdTooltip label="Double-click to rename">
            <span className={styles.subscriptionName} onDoubleClick={() => controller.rename(subscription.id, subscription.name)}>
              {subscription.name}
            </span>
          </KbdTooltip>
          <span className={styles.subscriptionStatus}>
            <StatusBadge
              tone={statusTone}
              label={statusLabel}
              title={state === 'attention' ? concreteProblem ?? 'Run interrupted' : undefined}
            />
          </span>
        </div>

        <div className={styles.subscriptionActions}>
          {running ? (
            <ActionButton variant="secondary" disabled={busy} onClick={() => controller.stop(subscription.id)}>
              <IconPlayerStop size={14} /> Stop
            </ActionButton>
          ) : (
            <ActionButton variant="primary" disabled={busy || subscription.paused || subscription.queries.length === 0} onClick={() => controller.run(subscription.id)}>
              <IconPlayerPlay size={14} /> Run now
            </ActionButton>
          )}
          <ActionButton variant="secondary" disabled={busy} onClick={() => controller.pause(subscription.id, !subscription.paused)}>
            {subscription.paused ? <IconPlayerPlay size={14} /> : <IconPlayerPause size={14} />} {subscription.paused ? 'Resume' : 'Pause'}
          </ActionButton>
          <OverflowMenuButton onOpen={onOpenMenu} />
        </div>

        <div
          className={`${styles.subscriptionRunProgress} ${running ? styles.subscriptionRunProgressActive : ''}`.trim()}
          aria-hidden={!running}
        >
          <div className={styles.subscriptionRunProgressLabel}>
            <span>{runTraversed.toLocaleString()} / {runTarget.toLocaleString()} posts traversed</span>
            <span>{(progress?.media_added ?? 0).toLocaleString()} media added</span>
          </div>
          <div
            className={styles.subscriptionRunProgressTrack}
            role={running ? 'progressbar' : undefined}
            aria-valuemin={running ? 0 : undefined}
            aria-valuemax={running ? runTarget : undefined}
            aria-valuenow={running ? runTraversed : undefined}
          >
            <span style={{ width: `${runProgressPercent}%` }} />
          </div>
        </div>

        <div className={styles.subscriptionProperties}>
          <div className={styles.subscriptionProperty}><span>Posts traversed</span><strong>{traversedCount.toLocaleString()}</strong></div>
          <div className={styles.subscriptionProperty}><span>Posts added</span><strong>{postsAddedCount.toLocaleString()}</strong></div>
          <div className={styles.subscriptionProperty}><span>Files downloaded</span><strong>{(progress?.files_downloaded ?? subscription.total_files).toLocaleString()}</strong></div>
          <div className={styles.subscriptionProperty}><span>Media added</span><strong>{(progress?.media_added ?? subscription.total_files).toLocaleString()}</strong></div>
          <div className={styles.subscriptionProperty}><span>Last check</span><strong>{formatRelativeTime(lastCheck)}</strong></div>
          <div className={styles.subscriptionProperty}>
            <span>Schedule</span>
            <CmSelect value={subscription.schedule} options={SCHEDULE_OPTIONS} onChange={(schedule) => controller.setSchedule(subscription.id, schedule)} width={100} />
          </div>
          <div className={styles.subscriptionProperty}>
            <span>Posts per run</span>
            <PostsPerRunInput
              value={subscription.posts_per_run}
              disabled={busy || running}
              onCommit={(postsPerRun) => controller.setPostsPerRun(subscription.id, postsPerRun)}
            />
          </div>
        </div>

        <div className={styles.subscriptionDestination}>
          <span className={styles.subscriptionRailHeading}>New files</span>
          <div className={styles.subscriptionRailField}>
            <span>Automatically add to folders</span>
            <div className={styles.subscriptionDestinationValues}>
              {selectedFolders.map((folder) => {
                const folderId = Number(folder.id.slice('folder:'.length));
                return <TagChip key={folder.id} namespace="" subtag={folder.name} colorRgb={hexToRgb(folder.color ?? null)} onRemove={() => void controller.setDestination(subscription.id, {
                  target_folder_ids: subscription.target_folder_ids.filter((id) => id !== folderId),
                  automatic_tags: subscription.automatic_tags,
                })} />;
              })}
              <button
                type="button"
                className={selectedFolders.length === 0 ? styles.subscriptionDestinationEmpty : styles.subscriptionDestinationAdd}
                onClick={(event) => {
                  const rect = event.currentTarget.getBoundingClientRect();
                  openFolderPicker({
                    open: true,
                    anchor: { x: rect.left, y: rect.top },
                    anchorPlacement: 'above',
                    selectedFolderIds: subscription.target_folder_ids,
                    onApplyFolders: (target_folder_ids) => void controller.setDestination(subscription.id, { target_folder_ids, automatic_tags: subscription.automatic_tags }),
                  });
                }}
              >
                <IconPlus size={14} />{selectedFolders.length === 0 && <span>Add to folders</span>}
              </button>
            </div>
          </div>
          <TagAssignmentControl
            tags={subscription.automatic_tags}
            onRemove={(tag) => void controller.setDestination(subscription.id, {
              target_folder_ids: subscription.target_folder_ids,
              automatic_tags: subscription.automatic_tags.filter((current) => current !== tag),
            })}
            onOpen={(button) => {
              const rect = button.getBoundingClientRect();
              openTagPicker({
                open: true,
                anchor: { x: rect.left, y: rect.top },
                anchorPlacement: 'above',
                selectedTags: subscription.automatic_tags,
                onApplyTags: (automatic_tags) => void controller.setDestination(subscription.id, { target_folder_ids: subscription.target_folder_ids, automatic_tags }),
              });
            }}
          />
        </div>
      </aside>

      <section className={styles.subscriptionDataPlane}>
        <header className={styles.subscriptionDataHeader}>
          <div className={styles.subscriptionTabs} role="tablist" aria-label="Subscription details">
            {tabs.map((entry) => (
              <button
                key={entry.id}
                type="button"
                role="tab"
                aria-selected={tab === entry.id}
                className={`${styles.subscriptionTab} ${tab === entry.id ? styles.subscriptionTabActive : ''}`.trim()}
                onClick={() => setTab(entry.id)}
              >
                {entry.label}
              </button>
            ))}
          </div>
        </header>

        <div className={styles.subscriptionDataBody}>
          {tab === 'sources' && (
            <div className={styles.subscriptionTableViewport} role="tabpanel">
              {subscription.queries.length > 0 ? (
                <div className={`${styles.qTable} ${styles.subscriptionTable}`.trim()}>
                  <div className={`${styles.subscriptionTableRow} ${styles.subscriptionTableHeader} ${styles.qRow}`}>
                    <span>Source</span>
                    <span className={styles.qCellSite}>Site</span>
                    <span className={styles.qCellNum}>Posts added</span>
                    <span className={styles.qCellNum}>Media added</span>
                    <span>Last check</span>
                    <span />
                  </div>
                  {subscription.queries.map((query) => {
                    const auth = getQueryAuthState({ query, sites: snapshot.sites, credentials: snapshot.credentials, credentialHealth: snapshot.credentialHealth });
                    const queryRunning = running && !query.paused;
                    return (
                      <QueryRow
                        key={query.id}
                        query={query}
                        sites={snapshot.sites}
                        running={queryRunning}
                        paused={query.paused}
                        authWarning={auth.blocking ? auth.label : null}
                        busy={busy}
                        onPause={(paused) => controller.pauseQuery(query.id, paused)}
                        onGrouping={(groupPosts) => controller.setQueryGrouping(query.id, groupPosts)}
                        onEdit={() => setEditing(query)}
                        onDelete={() => controller.deleteQuery(query.id)}
                        onOpenAuth={() => onOpenAccounts(query.site_id)}
                        onShowStats={() => setStatsQuery(query)}
                      />
                    );
                  })}
                </div>
              ) : (
                <div className={styles.subscriptionWorkspaceEmpty}>No sources yet.</div>
              )}
            </div>
          )}

          {tab === 'history' && (
            <div className={styles.subscriptionTableViewport} role="tabpanel">
              {detail.loading ? <div className={styles.subscriptionWorkspaceEmpty}>Loading history…</div> : <HistoryTab runs={detail.runs} />}
            </div>
          )}

          {tab === 'problems' && (
            <div className={styles.subscriptionTableViewport} role="tabpanel">
              {detail.loading ? (
                <div className={styles.subscriptionWorkspaceEmpty}>Checking for problems…</div>
              ) : problemCount === 0 ? (
                <div className={styles.subscriptionWorkspaceEmpty}>No problems found.</div>
              ) : (
                <HealthTab
                  failedPosts={detail.failedPosts}
                  issues={detail.issues}
                  busy={busy}
                  onOpenUrl={controller.openExternalUrl}
                  onFixCredentials={(issue) => {
                    const query = subscription.queries.find((entry) => Number(entry.id) === issue.query_id);
                    onOpenAccounts(query?.site_id ?? null);
                  }}
                  onReviewQuery={(issue) => {
                    const query = subscription.queries.find((entry) => Number(entry.id) === issue.query_id);
                    if (query) setEditing(query);
                  }}
                  failedPostTotalCount={detail.failedPostTotalCount}
                  issueTotalCount={detail.issueTotalCount}
                  retryablePostCount={detail.retryablePostCount}
                  hasMore={detail.issueNextCursor != null || detail.failedPostNextCursor != null}
                  onLoadMore={onLoadMoreHealth}
                />
              )}
            </div>
          )}
        </div>

        <footer className={styles.subscriptionDataFooter}>
          {tab === 'sources' && (
            <AddQueryBar sites={snapshot.sites} busy={busy} onAdd={(siteId, queryText) => controller.addQuery(subscription.id, siteId, queryText)} />
          )}
        </footer>
      </section>

      <QueryEditModal
        query={editing}
        sites={snapshot.sites}
        busy={busy}
        onClose={() => setEditing(null)}
        onSave={async (input) => {
          if (!editing) return;
          await controller.editQuery(Number.parseInt(editing.id, 10), input.siteId, input.queryText, input.displayName, input.notes);
          setEditing(null);
        }}
      />
      <GlassModal open={statsQuery != null} onClose={() => setStatsQuery(null)} title="Source details" size="sm">
        {statsQuery && (
          <div className={styles.queryStats}>
            <div><span>Source</span><strong>{statsQuery.display_name?.trim() || statsQuery.query_text}</strong></div>
            <div><span>Site</span><strong>{getSiteLabel(statsQuery.site_id, snapshot.sites)}</strong></div>
            <div><span>Posts added</span><strong>{statsQuery.posts_found.toLocaleString()}</strong></div>
            <div><span>Media added</span><strong>{statsQuery.files_found.toLocaleString()}</strong></div>
            <div><span>Last check</span><strong>{statsQuery.last_check_time ? formatRelativeTime(statsQuery.last_check_time) : 'Never'}</strong></div>
            <div><span>State</span><strong>{statsQuery.paused ? 'Paused' : statsQuery.completed_initial_run ? 'Ready' : 'Initial sync'}</strong></div>
            {statsQuery.last_failure_message && <div><span>Last error</span><strong>{statsQuery.last_failure_message}</strong></div>}
          </div>
        )}
      </GlassModal>
    </div>
  );
}
