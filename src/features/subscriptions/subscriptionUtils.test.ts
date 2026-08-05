import { describe, expect, it } from 'vitest';
import type { SubscriptionInfo, SubscriptionQueryInfo } from '../../shared/types/subscriptions';
import { isQueryUpToDate, isSubscriptionUpToDate } from './subscriptionUtils';

function query(overrides: Partial<SubscriptionQueryInfo> = {}): SubscriptionQueryInfo {
  return {
    id: '1',
    site_id: 'gelbooru',
    query_kind: 'search',
    query_text: 'huffslove',
    display_name: null,
    notes: null,
    paused: false,
    last_check_time: '2026-08-05T19:23:40Z',
    files_found: 198,
    posts_found: 198,
    completed_initial_run: true,
    resume_cursor: null,
    resume_strategy: null,
    last_success_at: '2026-08-05T19:23:40Z',
    last_failure_at: null,
    last_failure_kind: null,
    last_failure_message: null,
    ...overrides,
  };
}

function subscription(queries: SubscriptionQueryInfo[]): SubscriptionInfo {
  return {
    id: '1',
    name: 'Huffslove',
    schedule: 'daily',
    paused: false,
    group_id: null,
    initial_post_limit: 100,
    periodic_post_limit: 50,
    auto_collections: true,
    created_at: '2026-08-05T19:08:25Z',
    total_files: 198,
    queries,
  };
}

describe('subscription freshness', () => {
  it('marks a successfully exhausted query up to date', () => {
    expect(isQueryUpToDate(query())).toBe(true);
  });

  it('rejects incomplete, failed, paused, and unhealthy queries', () => {
    expect(isQueryUpToDate(query({ completed_initial_run: false }))).toBe(false);
    expect(isQueryUpToDate(query({ last_failure_kind: 'network' }))).toBe(false);
    expect(isQueryUpToDate(query({ paused: true }))).toBe(false);
    expect(isQueryUpToDate(query(), 1)).toBe(false);
  });

  it('requires every query and the subscription health to be current', () => {
    const current = subscription([query(), query({ id: '2' })]);
    expect(isSubscriptionUpToDate(current)).toBe(true);
    expect(isSubscriptionUpToDate(current, 1, 0)).toBe(false);
    expect(isSubscriptionUpToDate(current, 0, 1)).toBe(false);
    expect(isSubscriptionUpToDate(subscription([]))).toBe(false);
    expect(isSubscriptionUpToDate(subscription([query({ completed_initial_run: false })]))).toBe(false);
  });
});
