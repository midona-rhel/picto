import { describe, expect, it } from 'vitest';
import type {
  SubscriptionInfo,
  SubscriptionQueryInfo,
  SubscriptionSiteInfo,
} from '../../shared/types/subscriptions';
import {
  getQueryAuthState,
  isQueryUpToDate,
  isSubscriptionUpToDate,
} from './subscriptionUtils';
import { getCredentialOwnerSiteId } from '../../shared/lib/subscriptionHelpers';

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

describe('subscription account routing', () => {
  const sites = [
    { id: 'pixiv', credential_owner_site_id: 'pixiv' },
    { id: 'pixivuser', credential_owner_site_id: 'pixiv' },
    { id: 'gelbooru', credential_owner_site_id: 'gelbooru' },
  ] as SubscriptionSiteInfo[];

  it('routes source variants to their canonical credential owner', () => {
    expect(getCredentialOwnerSiteId('pixivuser', sites)).toBe('pixiv');
    expect(getCredentialOwnerSiteId('pixiv', sites)).toBe('pixiv');
    expect(getCredentialOwnerSiteId('gelbooru', sites)).toBe('gelbooru');
  });

  it('preserves unknown sources', () => {
    expect(getCredentialOwnerSiteId('future-source', sites)).toBe('future-source');
  });
});

describe('subscription auth state', () => {
  const authSite = (overrides: Partial<SubscriptionSiteInfo> = {}): SubscriptionSiteInfo => ({
    id: 'source',
    name: 'Source',
    domain: 'source.example',
    credential_owner_site_id: 'source',
    example_query: 'artist',
    supports_query: true,
    supports_account: false,
    auth_required_for_full_access: false,
    auth_strictly_required: false,
    credential_types: ['cookies'],
    ...overrides,
  });

  it('allows anonymous runs when login only improves coverage', () => {
    expect(getQueryAuthState({
      query: query({ site_id: 'source' }),
      sites: [authSite({ auth_required_for_full_access: true })],
      credentials: [],
      credentialHealth: [],
    })).toEqual({ tone: 'attention', label: 'Auth recommended', blocking: false });
  });

  it('blocks only strictly gated sources or known-bad saved credentials', () => {
    expect(getQueryAuthState({
      query: query({ site_id: 'source' }),
      sites: [authSite({ auth_required_for_full_access: true, auth_strictly_required: true })],
      credentials: [],
      credentialHealth: [],
    }).blocking).toBe(true);
  });
});
