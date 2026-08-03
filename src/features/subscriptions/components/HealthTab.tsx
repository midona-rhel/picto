import { useMemo, useState } from 'react';
import { IconExternalLink, IconRefresh } from '@tabler/icons-react';
import type { FailedPostGroup, SubscriptionIssueRecord } from '../../../shared/types/subscriptions';
import { KbdTooltip } from '../../../shared/ui/KbdTooltip/KbdTooltip';
import { ActionButton } from './ActionButton';
import { EmptyState } from './EmptyState';
import { StatusBadge } from './StatusBadge';
import { formatRelativeTime } from '../subscriptionUtils';
import styles from '../SubscriptionsScreen.module.css';

/** Rows rendered when a failure group is first expanded (rest behind "show all"). */
const EXPANDED_POST_CAP = 50;

function groupByReason(posts: FailedPostGroup[]): Map<string, FailedPostGroup[]> {
  const groups = new Map<string, FailedPostGroup[]>();
  for (const post of posts) {
    const reason = post.lastError?.trim() || 'Unknown failure';
    const key = reason.length > 80 ? `${reason.slice(0, 80)}…` : reason;
    const bucket = groups.get(key);
    if (bucket) bucket.push(post);
    else groups.set(key, [post]);
  }
  return groups;
}

/** Failed posts grouped by failure reason (bulk retry) + open issues. */
export function HealthTab({
  failedPosts,
  issues,
  busy,
  onRetryPosts,
  onOpenUrl,
}: {
  failedPosts: FailedPostGroup[];
  issues: SubscriptionIssueRecord[];
  busy: boolean;
  onRetryPosts: (posts: FailedPostGroup[]) => void;
  onOpenUrl: (url: string) => void;
}) {
  const groups = useMemo(() => groupByReason(failedPosts), [failedPosts]);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [showAllExpanded, setShowAllExpanded] = useState(false);
  const openIssues = issues.filter((issue) => issue.status !== 'resolved');
  const retryable = failedPosts.filter((post) => post.canRetry);

  if (failedPosts.length === 0 && openIssues.length === 0) {
    return <EmptyState title="All healthy" description="No failed posts and no open issues." />;
  }

  return (
    <div className={styles.healthStack}>
      {failedPosts.length > 0 && (
        <div className={styles.section}>
          <div className={styles.sectionHeader}>
            <span className={styles.subsectionTitle}>
              Failed posts ({failedPosts.length})
            </span>
            <ActionButton
              variant="secondary"
              compact
              disabled={busy || retryable.length === 0}
              onClick={() => onRetryPosts(retryable)}
            >
              <IconRefresh size={13} /> Retry all ({retryable.length})
            </ActionButton>
          </div>
          {[...groups.entries()].map(([reason, posts]) => {
            const groupRetryable = posts.filter((post) => post.canRetry);
            const isOpen = expanded === reason;
            return (
              <div key={reason} className={styles.healthGroup}>
                <div className={styles.healthGroupHeader}>
                  <button
                    type="button"
                    className={styles.linkButton}
                    onClick={() => setExpanded(isOpen ? null : reason)}
                  >
                    {posts.length} post{posts.length === 1 ? '' : 's'}
                  </button>
                  <span className={styles.healthGroupReason} title={reason}>{reason}</span>
                  <ActionButton
                    variant="ghost"
                    compact
                    disabled={busy || groupRetryable.length === 0}
                    onClick={() => onRetryPosts(groupRetryable)}
                  >
                    Retry
                  </ActionButton>
                </div>
                {isOpen && (showAllExpanded ? posts : posts.slice(0, EXPANDED_POST_CAP)).map((post) => (
                  <div key={post.key} className={styles.healthRow}>
                    <span>{post.queryLabel}</span>
                    <span className={styles.muted}>post {post.postId}</span>
                    <span className={styles.muted}>{post.failedMembers} file{post.failedMembers === 1 ? '' : 's'}</span>
                    <span className={styles.muted}>{post.retryCount} retr{post.retryCount === 1 ? 'y' : 'ies'}</span>
                    {post.nextRetryAt && (
                      <span className={styles.muted}>next {formatRelativeTime(post.nextRetryAt)}</span>
                    )}
                    {post.canonicalPostUrl && (
                      <KbdTooltip label="Open post">
                        <button
                          type="button"
                          className={styles.querySmallBtn}
                          onClick={() => onOpenUrl(post.canonicalPostUrl as string)}
                        >
                          <IconExternalLink size={13} />
                        </button>
                      </KbdTooltip>
                    )}
                  </div>
                ))}
                {isOpen && !showAllExpanded && posts.length > EXPANDED_POST_CAP && (
                  <button
                    type="button"
                    className={styles.historyMore}
                    onClick={() => setShowAllExpanded(true)}
                  >
                    Show all {posts.length} posts
                  </button>
                )}
              </div>
            );
          })}
        </div>
      )}

      {openIssues.length > 0 && (
        <div className={styles.section}>
          <div className={styles.sectionHeader}>
            <span className={styles.subsectionTitle}>Open issues ({openIssues.length})</span>
          </div>
          {openIssues.map((issue) => (
            <div key={issue.issue_id} className={styles.issueCard}>
              <div className={styles.issueHeader}>
                <StatusBadge tone="attention" label={issue.issue_kind.split('_').join(' ')} />
                <span className={styles.muted}>{formatRelativeTime(issue.last_seen_at)}</span>
              </div>
              <div>{issue.message}</div>
              {issue.detail && <div className={styles.muted}>{issue.detail}</div>}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
