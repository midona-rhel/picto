import {
  IconCircleCheckFilled,
  IconCopy,
  IconPencil,
  IconPlayerPause,
  IconPlayerPlay,
  IconTrash,
} from '@tabler/icons-react';
import type {
  SubscriptionQueryInfo,
  SubscriptionSiteInfo,
} from '../../../shared/types/subscriptions';
import { KbdTooltip } from '../../../shared/ui/KbdTooltip/KbdTooltip';
import { formatRelativeTime, getSiteLabel } from '../subscriptionUtils';
import styles from '../SubscriptionsScreen.module.css';

/** One dense table row in the queries table. */
export function QueryRow({
  query,
  sites,
  running,
  paused,
  authWarning,
  busy,
  onPause,
  onGrouping,
  onEdit,
  onDelete,
  onOpenAuth,
  onShowStats,
}: {
  query: SubscriptionQueryInfo;
  sites: SubscriptionSiteInfo[];
  running: boolean;
  paused: boolean;
  /** Non-null when the site needs auth attention; text shown on the chip. */
  authWarning: string | null;
  busy: boolean;
  onPause: (paused: boolean) => void;
  onGrouping: (groupPosts: boolean) => void;
  onEdit: () => void;
  onDelete: () => void;
  onOpenAuth: () => void;
  onShowStats: () => void;
}) {
  const label = query.display_name?.trim() || query.query_text;

  return (
    <KbdTooltip label="Double-click for source details"><div className={`${styles.subscriptionTableRow} ${styles.qRow}`.trim()} onDoubleClick={onShowStats}>
      <span className={styles.qCellName}>
        <span className={styles.qName} title={query.query_text}>{label}</span>
        {query.source_history_complete && query.completed_initial_run && (
          <IconCircleCheckFilled className={styles.qCompleteIcon} size={13} title="Checked all available posts" />
        )}
        {authWarning && <button type="button" className={styles.qAuthChip} onClick={onOpenAuth}>{authWarning}</button>}
        {query.last_failure_message && !running && (
          <span className={styles.qFailureText} title={query.last_failure_message}>{query.last_failure_message}</span>
        )}
      </span>
      <span className={styles.qCellSite}>{getSiteLabel(query.site_id, sites)}</span>
      <span className={styles.qCellNum}>{query.posts_found.toLocaleString()}</span>
      <span className={styles.qCellNum}>{query.files_found.toLocaleString()}</span>
      <span className={styles.qCellTime}>
        {query.last_check_time ? formatRelativeTime(query.last_check_time) : 'never'}
        {!query.completed_initial_run && ' · syncing'}
      </span>
      <span className={styles.qCellActions}>
        <KbdTooltip label={query.group_posts ? 'Group multi-media posts' : 'Keep post media separate'}>
          <button
            type="button"
            className={`${styles.querySmallBtn} ${query.group_posts ? styles.querySmallBtnActive : ''}`.trim()}
            aria-label="Group multi-media posts"
            aria-pressed={query.group_posts}
            onClick={() => onGrouping(!query.group_posts)}
            disabled={busy || running}
          >
            <IconCopy size={14} />
          </button>
        </KbdTooltip>
        <KbdTooltip label={paused ? 'Resume query' : 'Pause query'}>
          <button
            type="button"
            className={styles.querySmallBtn}
            onClick={() => onPause(!paused)}
            disabled={busy}
          >
            {paused ? <IconPlayerPlay size={14} /> : <IconPlayerPause size={14} />}
          </button>
        </KbdTooltip>
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
    </div></KbdTooltip>
  );
}
