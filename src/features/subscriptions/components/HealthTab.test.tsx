import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { FailedPostGroup, SubscriptionIssueRecord } from '../../../shared/types/subscriptions';
import { HealthTab } from './HealthTab';

function issue(
  issueId: number,
  recoveryAction: SubscriptionIssueRecord['recovery_action'],
): SubscriptionIssueRecord {
  return {
    issue_id: issueId,
    issue_key: `query:1:${recoveryAction}`,
    subscription_id: 1,
    query_id: 1,
    issue_kind: recoveryAction,
    status: 'open',
    message: `Problem ${issueId}`,
    detail: null,
    first_seen_at: '2026-08-06T00:00:00Z',
    last_seen_at: '2026-08-06T00:00:00Z',
    resolved_at: null,
    recovery_action: recoveryAction,
    next_retry_at: recoveryAction === 'retry_automatically' ? '2026-08-07T00:00:00Z' : null,
  };
}

const failedPost: FailedPostGroup = {
  key: '1:gelbooru:42',
  queryId: '1',
  queryLabel: 'one_girl',
  siteId: 'gelbooru',
  postId: '42',
  canonicalPostUrl: null,
  mediaUrl: null,
  failedMembers: 1,
  retryCount: 1,
  status: 'pending',
  lastError: 'Download failed',
  nextRetryAt: null,
};

describe('HealthTab', () => {
  it('binds persisted recovery actions without retrying the displayed subset', async () => {
    const user = userEvent.setup();
    const onRetryAll = vi.fn();
    const onFixCredentials = vi.fn();
    const onReviewQuery = vi.fn();

    render(
      <HealthTab
        failedPosts={[failedPost]}
        issues={[
          issue(1, 'retry_now'),
          issue(2, 'fix_credentials'),
          issue(3, 'review_query'),
          issue(4, 'retry_automatically'),
        ]}
        busy={false}
        onRetryAll={onRetryAll}
        onRetryPost={vi.fn()}
        onOpenUrl={vi.fn()}
        onFixCredentials={onFixCredentials}
        onReviewQuery={onReviewQuery}
        failedPostTotalCount={250}
        issueTotalCount={4}
        retryablePostCount={250}
        hasMore
        onLoadMore={vi.fn()}
      />,
    );

    expect(screen.getByText('Failed posts (250)')).toBeInTheDocument();
    expect(screen.getByText(/Retries automatically/)).toBeInTheDocument();
    expect(screen.getAllByRole('button', { name: 'Retry all' })).toHaveLength(1);

    await user.click(screen.getByRole('button', { name: 'Retry all' }));
    await user.click(screen.getByRole('button', { name: 'Fix login' }));
    await user.click(screen.getByRole('button', { name: 'Review query' }));

    expect(onRetryAll).toHaveBeenCalledOnce();
    expect(onFixCredentials).toHaveBeenCalledWith(expect.objectContaining({ issue_id: 2 }));
    expect(onReviewQuery).toHaveBeenCalledWith(expect.objectContaining({ issue_id: 3 }));
  });
});
