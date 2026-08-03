import {
  IconAlertTriangle,
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
import { describeFailure, formatRelativeTime, getSiteLabel } from '../subscriptionUtils';
import styles from '../SubscriptionsScreen.module.css';

/** One dense table row in the queries table. */
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
  const failureText = !running && !paused
    ? describeFailure(query.last_failure_kind, query.last_failure_message)
    : null;
  const hasFailure = failedCount > 0 || (query.last_failure_kind != null && !running);
  const tone = running ? 'running' : paused ? 'paused' : hasFailure ? 'attention' : 'idle';
  const toneLabel = running
    ? progress
      ? `${progress.files_downloaded} down · ${progress.ingested} in`
      : 'Running'
    : paused
      ? 'Paused'
      : failedCount > 0
        ? `${failedCount} failed`
        : query.last_failure_kind
          ? failureText ?? 'Failed'
          : 'Idle';
  const dotClass =
    tone === 'running'
      ? styles.qDotRunning
      : tone === 'paused'
        ? styles.qDotPaused
        : tone === 'attention'
          ? styles.qDotAttention
          : styles.qDotIdle;

  return (
    <div className={styles.qRow}>
      <span className={styles.qCellName}>
        <span className={styles.qName} title={query.query_text}>{label}</span>
        {authWarning && (
          <KbdTooltip label={`${authWarning} — click to manage account`}>
            <button type="button" className={styles.qAuthChip} onClick={onOpenAuth}>
              <IconShieldLock size={12} />
            </button>
          </KbdTooltip>
        )}
        {query.last_failure_message && !running && (
          <KbdTooltip label={query.last_failure_message}>
            <span className={styles.qFailIcon}><IconAlertTriangle size={12} /></span>
          </KbdTooltip>
        )}
      </span>
      <span className={styles.qCellSite}>{getSiteLabel(query.site_id, sites)}</span>
      <span className={styles.qCellNum}>{query.posts_found.toLocaleString()}</span>
      <span className={styles.qCellNum}>{query.files_found.toLocaleString()}</span>
      <span className={styles.qCellTime}>
        {query.last_check_time ? formatRelativeTime(query.last_check_time) : 'never'}
        {!query.completed_initial_run && ' · syncing'}
      </span>
      <span
        className={styles.qCellStatus}
        title={query.last_failure_message ?? undefined}
      >
        <span className={`${styles.qDot} ${dotClass}`.trim()} />
        {toneLabel}
      </span>
      <span className={styles.qCellActions}>
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
  );
}
