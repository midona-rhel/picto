import { IconExternalLink, IconPencil, IconRefresh, IconShieldLock } from '@tabler/icons-react';
import type { FailedPostGroup, SubscriptionIssueRecord } from '../../../shared/types/subscriptions';
import { KbdTooltip } from '../../../shared/ui/KbdTooltip/KbdTooltip';
import { ActionButton } from './ActionButton';
import { EmptyState } from './EmptyState';
import { StatusBadge } from './StatusBadge';
import { formatRelativeTime } from '../subscriptionUtils';
import styles from '../SubscriptionsScreen.module.css';

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
  const openIssues = issues.filter((issue) => issue.status !== 'resolved');

  if (failedPostTotalCount === 0 && issueTotalCount === 0) {
    return <EmptyState title="All healthy" description="No failed posts and no open issues." />;
  }

  return (
    <div className={`${styles.subscriptionTable} ${styles.problemTable}`.trim()}>
      <div className={`${styles.subscriptionTableRow} ${styles.subscriptionTableHeader} ${styles.problemRow}`.trim()}>
        <span>Status</span><span>Post</span><span>Why it failed</span><span>Last seen</span><span />
      </div>

      {failedPosts.map((post) => {
        const reason = post.lastError?.trim() || 'Unknown failure';
        const postLabel = post.postId || post.mediaUrl || 'Unknown post';
        return (
          <div key={post.key} className={`${styles.subscriptionTableRow} ${styles.problemRow}`.trim()}>
            <span className={styles.qCellStatus}><StatusBadge tone="attention" label={`${post.failedMembers} failed`} /></span>
            <span className={styles.problemTarget}>
              {post.canonicalPostUrl ? (
                <a href={post.canonicalPostUrl} onClick={(event) => { event.preventDefault(); onOpenUrl(post.canonicalPostUrl as string); }}>{postLabel}</a>
              ) : <span>{postLabel}</span>}
            </span>
            <span className={styles.problemMessage} title={reason}>{reason}</span>
            <span className={styles.qCellTime}>{post.nextRetryAt ? formatRelativeTime(post.nextRetryAt) : '—'}</span>
            <span className={styles.qCellActions}>
              {post.canonicalPostUrl && <KbdTooltip label="Open post"><button type="button" aria-label="Open post" className={styles.querySmallBtn} onClick={() => onOpenUrl(post.canonicalPostUrl as string)}><IconExternalLink size={13} /></button></KbdTooltip>}
              {onRetryPost && <KbdTooltip label="Retry"><button type="button" aria-label="Retry failed post" className={styles.querySmallBtn} disabled={busy} onClick={() => onRetryPost(post)}><IconRefresh size={13} /></button></KbdTooltip>}
            </span>
          </div>
        );
      })}

      {openIssues.map((issue) => (
        <div key={issue.issue_id} className={`${styles.subscriptionTableRow} ${styles.problemRow}`.trim()}>
          <span className={styles.qCellStatus}><StatusBadge tone="attention" label={issue.issue_kind === 'download_item' ? 'download' : issue.issue_kind.split('_').join(' ')} /></span>
          <span className={styles.problemTarget}>
            {issue.canonical_post_url ? (
              <a href={issue.canonical_post_url} onClick={(event) => { event.preventDefault(); onOpenUrl(issue.canonical_post_url as string); }}>{issue.source_post_title?.trim() || issue.source_post_key || issue.source_item_key || 'Unknown post'}</a>
            ) : <span>{issue.source_post_title?.trim() || issue.source_post_key || issue.source_item_key || 'Subscription'}</span>}
          </span>
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
