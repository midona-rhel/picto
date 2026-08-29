import { act, render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
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
    source_item_key: null,
    source_post_key: null,
    source_post_title: null,
    canonical_post_url: null,
    media_url: null,
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
    const click = (...args: Parameters<typeof user.click>) => act(() => user.click(...args));
    const onRetryAll = vi.fn();
    const onFixCredentials = vi.fn();
    const onReviewQuery = vi.fn();

    render(
      <MantineProvider>
        <HealthTab
        failedPosts={[failedPost]}
        issues={[
          {
            ...issue(1, 'retry_now'),
            source_item_key: 'attachment-1',
            source_post_key: 'post-42',
            source_post_title: 'Broken Patreon post',
            canonical_post_url: 'https://www.patreon.com/posts/42',
            media_url: 'https://cdn.example.invalid/deleted.png',
            issue_kind: 'download_item',
          },
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
        />
      </MantineProvider>,
    );

    expect(screen.getByText('Status')).toBeInTheDocument();
    expect(screen.getByText('Post')).toBeInTheDocument();
    expect(screen.getByText('Why it failed')).toBeInTheDocument();
    expect(screen.getByText('1 failed')).toBeInTheDocument();
    expect(screen.getByText('download')).toBeInTheDocument();
    expect(screen.getByText('Download failed')).toHaveAttribute('title', 'Download failed');
    expect(screen.getByRole('link', { name: 'Broken Patreon post' })).toHaveAttribute('href', 'https://www.patreon.com/posts/42');
    expect(screen.queryByRole('link', { name: 'https://cdn.example.invalid/deleted.png' })).not.toBeInTheDocument();
    expect(screen.getByText('retry automatically')).toBeInTheDocument();
    expect(screen.getAllByRole('button', { name: 'Retry all' })).toHaveLength(1);

    await click(screen.getByRole('button', { name: 'Retry all' }));
    await click(screen.getByRole('button', { name: 'Fix login' }));
    await click(screen.getByRole('button', { name: 'Review source' }));

    expect(onRetryAll).toHaveBeenCalledOnce();
    expect(onFixCredentials).toHaveBeenCalledWith(expect.objectContaining({ issue_id: 2 }));
    expect(onReviewQuery).toHaveBeenCalledWith(expect.objectContaining({ issue_id: 3 }));
  });
});
