import type {
  FailedPostGroup,
  SubscriptionDownloadAttemptRecord,
  SubscriptionProgressEvent,
  SubscriptionQueryInfo,
} from '../types/subscriptions';

export function getProgressBySubscriptionId(progress: SubscriptionProgressEvent[]): Map<string, SubscriptionProgressEvent> {
  return new Map(progress.map((entry) => [entry.subscription_id, entry]));
}

export function groupFailedPostAttempts(
  attempts: SubscriptionDownloadAttemptRecord[],
  queries: SubscriptionQueryInfo[],
): FailedPostGroup[] {
  const queryById = new Map(queries.map((query) => [query.id, query]));
  const grouped = new Map<string, FailedPostGroup>();

  for (const attempt of attempts) {
    if (!attempt.post_id || attempt.resolved_at) continue;
    if (attempt.status === 'resolved' || attempt.status === 'succeeded') continue;

    const queryId = attempt.query_id == null ? null : String(attempt.query_id);
    const query = queryId ? queryById.get(queryId) : null;
    const siteId = query?.site_id ?? attempt.site_category ?? 'unknown';
    const key = `${queryId ?? 'none'}:${attempt.post_id}`;
    const existing = grouped.get(key);
    const queryLabel = queryId
      ? query?.display_name?.trim() || query?.query_text || `Query ${queryId}`
      : 'Unknown query';

    if (!existing) {
      grouped.set(key, {
        key,
        queryId,
        queryLabel,
        siteId,
        postId: attempt.post_id,
        canonicalPostUrl: attempt.canonical_post_url,
        mediaUrl: attempt.media_url,
        failedMembers: 1,
        retryCount: attempt.retry_count,
        status: attempt.status,
        lastError: attempt.last_error,
        nextRetryAt: attempt.next_retry_at,
      });
      continue;
    }

    existing.failedMembers += 1;
    if (attempt.retry_count > existing.retryCount) existing.retryCount = attempt.retry_count;
    if (!existing.lastError && attempt.last_error) existing.lastError = attempt.last_error;
    if (!existing.canonicalPostUrl && attempt.canonical_post_url) existing.canonicalPostUrl = attempt.canonical_post_url;
    if (!existing.mediaUrl && attempt.media_url) existing.mediaUrl = attempt.media_url;
    if (!existing.nextRetryAt && attempt.next_retry_at) existing.nextRetryAt = attempt.next_retry_at;
  }

  return Array.from(grouped.values()).sort((left, right) => {
    const leftTime = left.nextRetryAt ?? '';
    const rightTime = right.nextRetryAt ?? '';
    if (leftTime !== rightTime) return rightTime.localeCompare(leftTime);
    return left.postId.localeCompare(right.postId);
  });
}
