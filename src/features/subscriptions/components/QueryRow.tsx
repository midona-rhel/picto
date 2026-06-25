import {
  IconPencil,
  IconPlayerPlay,
  IconPlayerStop,
  IconShieldLock,
  IconTrash,
} from '@tabler/icons-react';
import type {
  SubscriptionProgressEvent,
  SubscriptionQueryInfo,
  SubscriptionSiteInfo,
} from '../../../shared/types/subscriptions';
import { KbdTooltip } from '../../../shared/ui/KbdTooltip/KbdTooltip';
import { StatusBadge } from './StatusBadge';
import { formatRelativeTime, getSiteLabel } from '../subscriptionUtils';
import styles from '../SubscriptionsScreen.module.css';

export function QueryRow({
  query,
  sites,
  running,
  paused,
  failedCount,
  authWarning,
  progress,
  busy,
  onRun,
  onStop,
  onPause,
  onEdit,
  onDelete,
  onOpenAuth,
}: {
  query: SubscriptionQueryInfo;
  sites: SubscriptionSiteInfo[];
  running: boolean;
  paused: boolean;
  failedCount: number;
  /** Non-null when the site needs auth attention; text shown on the chip. */
  authWarning: string | null;
  progress: SubscriptionProgressEvent | null;
  busy: boolean;
  onRun: () => void;
  onStop: () => void;
  onPause: (paused: boolean) => void;
  onEdit: () => void;
  onDelete: () => void;
  onOpenAuth: () => void;
}) {
  const label = query.display_name?.trim() || query.query_text;
  const tone = running ? 'running' : paused ? 'paused' : failedCount > 0 ? 'attention' : 'idle';
  const toneLabel = running ? 'Running' : paused ? 'Paused' : failedCount > 0 ? `${failedCount} failed` : 'Idle';

  return (
    <div className={styles.queryCard}>
      <div className={styles.queryCardHeader}>
        <span className={styles.queryTitle}>{label}</span>
        <span className={styles.pill}>{getSiteLabel(query.site_id, sites)}</span>
        <StatusBadge tone={tone} label={toneLabel} />
        {authWarning && (
          <KbdTooltip label="Account needs attention — click to manage">
            <button type="button" className={styles.linkButton} onClick={onOpenAuth}>
              <IconShieldLock size={13} /> {authWarning}
            </button>
          </KbdTooltip>
        )}
        <span className={styles.queryCardActions}>
          {running ? (
            <KbdTooltip label="Stop">
              <button type="button" className={styles.querySmallBtn} onClick={onStop} disabled={busy}>
                <IconPlayerStop size={14} />
              </button>
            </KbdTooltip>
          ) : (
            <KbdTooltip label={paused ? 'Resume' : 'Run now'}>
              <button
                type="button"
                className={styles.querySmallBtn}
                onClick={() => (paused ? onPause(false) : onRun())}
                disabled={busy}
              >
                <IconPlayerPlay size={14} />
              </button>
            </KbdTooltip>
          )}
          <KbdTooltip label="Edit">
            <button type="button" className={styles.querySmallBtn} onClick={onEdit} disabled={busy}>
              <IconPencil size={14} />
            </button>
          </KbdTooltip>
          <KbdTooltip label="Delete">
            <button type="button" className={styles.querySmallBtn} onClick={onDelete} disabled={busy}>
              <IconTrash size={14} />
            </button>
          </KbdTooltip>
        </span>
      </div>
      <div className={styles.queryCardStats}>
        <span className={styles.queryMetaItem}>{query.posts_found.toLocaleString()} posts</span>
        <span className={styles.queryMetaItem}>{query.files_found.toLocaleString()} files</span>
        <span className={styles.queryMetaItem}>
          {query.last_check_time ? `checked ${formatRelativeTime(query.last_check_time)}` : 'never checked'}
        </span>
        {!query.completed_initial_run && <span className={styles.queryMetaItem}>initial sync incomplete</span>}
        {running && progress && (
          <span className={styles.queryMetaItem}>
            {progress.files_downloaded} down · {progress.ingested} imported
          </span>
        )}
        {query.last_failure_message && !running && (
          <span className={styles.queryMetaItem} title={query.last_failure_message}>
            ⚠ {query.last_failure_kind ?? 'failed'}
          </span>
        )}
      </div>
    </div>
  );
}
