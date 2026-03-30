import { IconExternalLink, IconRefresh } from '@tabler/icons-react';
import type { FailedPostGroup } from '../../../shared/types/subscriptions';
import { ActionButton } from './ActionButton';
import { EmptyState } from './EmptyState';
import { StatusBadge } from './StatusBadge';
import { formatRelativeTime } from '../subscriptionUtils';
import styles from '../SubscriptionsScreen.module.css';

export function FailedTab({
  failedPosts,
  loading,
  onOpenExternal,
  onRetryPost,
}: {
  failedPosts: FailedPostGroup[];
  loading: boolean;
  onOpenExternal: (url: string) => void;
  onRetryPost: (failedPost: FailedPostGroup) => Promise<void>;
}) {
  return (
    <div className={styles.section}>
      <div className={styles.helperCard}>
        Failed downloads are retried as whole posts. If the post already has a mapped collection, recovery reconciles into the existing collection and restores page order.
      </div>

      {loading && (
        <div className={styles.muted}>Loading failed posts…</div>
      )}

      {failedPosts.map((failedPost) => (
        <div key={failedPost.key} className={styles.failedCard}>
          <div className={styles.failedHeader}>
            <div className={styles.titleWrap}>
              <div className={styles.sectionTitle}>Post {failedPost.postId}</div>
              <div className={styles.muted}>{failedPost.queryLabel} · {failedPost.siteId}</div>
            </div>
            <StatusBadge tone="attention" label={failedPost.status} />
          </div>
          <div className={styles.failedStats}>
            <span className={styles.smallBadge}>{failedPost.failedMembers} missing members</span>
            <span className={styles.smallBadge}>retry #{failedPost.retryCount}</span>
            {failedPost.nextRetryAt && <span className={styles.smallBadge}>next {formatRelativeTime(failedPost.nextRetryAt)}</span>}
          </div>
          {failedPost.canonicalPostUrl && (
            <button className={styles.linkButton} onClick={() => onOpenExternal(failedPost.canonicalPostUrl!)}>
              {failedPost.canonicalPostUrl} <IconExternalLink size={12} />
            </button>
          )}
          <div className={styles.muted}>{failedPost.lastError ?? 'Download attempt failed without a retained error message.'}</div>
          <div className={styles.inlineActions}>
            <ActionButton
              variant="primary"
              compact
              disabled={!failedPost.canRetry}
              onClick={() => { void onRetryPost(failedPost); }}
            >
              <IconRefresh size={14} />
              Retry Post
            </ActionButton>
          </div>
        </div>
      ))}

      {!loading && failedPosts.length === 0 && (
        <EmptyState title="No failed posts" description="Whole-post retries will appear here when a download attempt is persisted as failed." />
      )}
    </div>
  );
}
