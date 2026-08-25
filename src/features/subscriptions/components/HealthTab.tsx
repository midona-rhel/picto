import { useMemo } from 'react';
import { IconExternalLink, IconPencil, IconRefresh, IconShieldLock } from '@tabler/icons-react';
import type { FailedPostGroup, SubscriptionIssueRecord } from '../../../shared/types/subscriptions';
import { KbdTooltip } from '../../../shared/ui/KbdTooltip/KbdTooltip';
import { ActionButton } from './ActionButton';
import { EmptyState } from './EmptyState';
import { formatRelativeTime } from '../subscriptionUtils';
import styles from '../SubscriptionsScreen.module.css';

function groupByReason(posts: FailedPostGroup[]): Map<string, FailedPostGroup[]> {
  const groups = new Map<string, FailedPostGroup[]>();
  for (const post of posts) {
    const reason = post.lastError?.trim() || 'Unknown failure';
    const bucket = groups.get(reason);
    if (bucket) bucket.push(post);
    else groups.set(reason, [post]);
  }
  return groups;
}

/** One compact table for failed posts and subscription issues. */
export function HealthTab({
  failedPosts,
  issues,
  busy,
  onRetryAll,
  onRetryPost,
  onOpenUrl,
  onFixCredentials,
  onReviewQuery,
  failedPostTotalCount,
  issueTotalCount,
  retryablePostCount,
  hasMore,
  onLoadMore,
}: {
  failedPosts: FailedPostGroup[];
  issues: SubscriptionIssueRecord[];
  busy: boolean;
  onRetryAll?: () => void;
  onRetryPost?: (post: FailedPostGroup) => void;
  onOpenUrl: (url: string) => void;
  onFixCredentials: (issue: SubscriptionIssueRecord) => void;
  onReviewQuery: (issue: SubscriptionIssueRecord) => void;
  failedPostTotalCount: number;
  issueTotalCount: number;
  retryablePostCount: number;
  hasMore: boolean;
  onLoadMore: () => void;
}) {
  const groups = useMemo(() => groupByReason(failedPosts), [failedPosts]);
  const openIssues = issues.filter((issue) => issue.status !== 'resolved');

  if (failedPostTotalCount === 0 && issueTotalCount === 0) {
    return <EmptyState title="All healthy" description="No failed posts and no open issues." />;
  }

  return (
    <div className={`${styles.subscriptionTable} ${styles.problemTable}`.trim()}>
      <div className={`${styles.subscriptionTableRow} ${styles.subscriptionTableHeader} ${styles.problemRow}`.trim()}>
        <span>Status</span><span>Error message</span><span>Last seen</span><span />
      </div>

      {[...groups.entries()].map(([reason, posts]) => {
        const first = posts[0];
        return (
          <div key={reason} className={`${styles.subscriptionTableRow} ${styles.problemRow}`.trim()}>
            <span className={styles.qCellStatus}><span className={`${styles.qDot} ${styles.qDotAttention}`} />{posts.length} failed</span>
            <span className={styles.problemMessage} title={reason}>{reason}</span>
            <span className={styles.qCellTime}>{first.nextRetryAt ? formatRelativeTime(first.nextRetryAt) : '—'}</span>
            <span className={styles.qCellActions}>
              {first.canonicalPostUrl && <KbdTooltip label="Open post"><button type="button" aria-label="Open post" className={styles.querySmallBtn} onClick={() => onOpenUrl(first.canonicalPostUrl as string)}><IconExternalLink size={13} /></button></KbdTooltip>}
              {onRetryPost && <KbdTooltip label="Retry"><button type="button" aria-label="Retry failed post" className={styles.querySmallBtn} disabled={busy} onClick={() => onRetryPost(first)}><IconRefresh size={13} /></button></KbdTooltip>}
            </span>
          </div>
        );
      })}

      {openIssues.map((issue) => (
        <div key={issue.issue_id} className={`${styles.subscriptionTableRow} ${styles.problemRow}`.trim()}>
          <span className={styles.qCellStatus}><span className={`${styles.qDot} ${styles.qDotAttention}`} />{issue.issue_kind.split('_').join(' ')}</span>
          <span className={styles.problemMessage} title={[issue.message, issue.detail].filter(Boolean).join('\n')}>{issue.message}</span>
          <span className={styles.qCellTime}>{formatRelativeTime(issue.last_seen_at)}</span>
          <span className={styles.qCellActions}>
            {issue.recovery_action === 'fix_credentials' && <KbdTooltip label="Fix login"><button type="button" aria-label="Fix login" className={styles.querySmallBtn} disabled={busy} onClick={() => onFixCredentials(issue)}><IconShieldLock size={13} /></button></KbdTooltip>}
            {issue.recovery_action === 'retry_now' && <KbdTooltip label="Retry now"><button type="button" aria-label="Retry now" className={styles.querySmallBtn} disabled={busy || !onRetryAll} onClick={() => onRetryAll?.()}><IconRefresh size={13} /></button></KbdTooltip>}
            {issue.recovery_action === 'review_query' && <KbdTooltip label="Review source"><button type="button" aria-label="Review source" className={styles.querySmallBtn} disabled={busy} onClick={() => onReviewQuery(issue)}><IconPencil size={13} /></button></KbdTooltip>}
          </span>
        </div>
      ))}

      {((retryablePostCount > 0 && onRetryAll) || hasMore) && (
        <div className={styles.problemTableActions}>
          {retryablePostCount > 0 && onRetryAll && <ActionButton variant="secondary" compact disabled={busy} onClick={onRetryAll}><IconRefresh size={13} /> Retry all</ActionButton>}
          {hasMore && <ActionButton variant="secondary" compact disabled={busy} onClick={onLoadMore}>Load more</ActionButton>}
        </div>
      )}
    </div>
  );
}
