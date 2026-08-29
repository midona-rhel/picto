import { describe, expect, it } from 'vitest';
import type {
  SubscriptionInfo,
  SubscriptionQueryInfo,
  SubscriptionSiteInfo,
} from '../../shared/types/subscriptions';
import {
  describeSubscriptionState,
  getQueryAuthState,
  getSubscriptionRunTarget,
  isQueryCompleted,
  isGalleryImportJob,
  isSubscriptionCompleted,
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
    group_posts: true,
    paused: false,
    last_check_time: '2026-08-05T19:23:40Z',
    files_found: 198,
    posts_found: 198,
    completed_initial_run: true,
    source_history_complete: true,
    successful_run_count: 2,
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
    run_status: null,
    created_at: '2026-08-05T19:08:25Z',
    total_items: 198,
    posts_per_run: 100,
    target_folder_ids: [],
    automatic_tags: [],
    queries,
  };
}

describe('subscription completion', () => {
  it('marks the first successful run complete', () => {
    const firstRun = query({ successful_run_count: 1 });
    expect(isQueryCompleted(firstRun)).toBe(true);
  });

  it('rejects incomplete, failed, and paused queries', () => {
    expect(isQueryCompleted(query({ completed_initial_run: false }))).toBe(false);
    expect(isQueryCompleted(query({ last_failure_kind: 'network' }))).toBe(false);
    expect(isQueryCompleted(query({ paused: true }))).toBe(false);
  });

  it('uses query completion independently from non-fatal media warnings', () => {
    const current = subscription([query(), query({ id: '2' })]);
    expect(isSubscriptionCompleted(current)).toBe(true);
    expect(isSubscriptionCompleted(subscription([
      query({ successful_run_count: 1 }),
    ]))).toBe(true);
    expect(isSubscriptionCompleted(subscription([]))).toBe(false);
    expect(isSubscriptionCompleted(subscription([query({ completed_initial_run: false })]))).toBe(false);
  });
});

describe('gallery imports', () => {
  it('classifies only the dedicated E-Hentai URL job', () => {
    expect(isGalleryImportJob(subscription([
      query({ site_id: 'ehentai', query_kind: 'user', query_text: 'https://e-hentai.org/g/1/0123456789/' }),
    ]))).toBe(true);
    expect(isGalleryImportJob(subscription([query({ site_id: 'twitter' })]))).toBe(false);
  });
});

describe('subscription state', () => {
  it('keeps an explicitly paused subscription paused while stale progress is present', () => {
    expect(describeSubscriptionState({
      paused: true,
      progress: {} as never,
      failedPostCount: 0,
      openIssueCount: 0,
    })).toBe('paused');
  });

  it('shows the actual run while future automatic runs are paused', () => {
    expect(describeSubscriptionState({
      paused: true,
      running: true,
      failedPostCount: 0,
      openIssueCount: 0,
    })).toBe('running');
  });
});

describe('subscription run target', () => {
  it('sums the per-query post limit across active queries', () => {
    const value = subscription([
      query({ id: 'initial', completed_initial_run: false }),
      query({ id: 'periodic', completed_initial_run: true }),
      query({ id: 'paused', paused: true }),
    ]);
    value.posts_per_run = 40;

    expect(getSubscriptionRunTarget(value)).toBe(80);
  });

  it('uses one hundred source posts per active query by default', () => {
    expect(getSubscriptionRunTarget(subscription([
      query({ id: 'one' }),
      query({ id: 'two' }),
      query({ id: 'paused', paused: true }),
    ]))).toBe(200);
  });

  it('does not invent a source-specific limit for account feeds', () => {
    const value = subscription([query({ site_id: 'twitter' })]);
    expect(getSubscriptionRunTarget(value)).toBe(100);
  });

  it('uses one query budget for a manually selected query run', () => {
    const value = subscription([query({ id: 'one' }), query({ id: 'two' })]);
    value.posts_per_run = 40;
    expect(getSubscriptionRunTarget(value, 'manual-query')).toBe(40);
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
    })).toEqual({ tone: 'idle', label: '', blocking: false });
  });

  it('blocks only strictly gated sources or known-bad saved credentials', () => {
    expect(getQueryAuthState({
      query: query({ site_id: 'source' }),
      sites: [authSite({ auth_required_for_full_access: true, auth_strictly_required: true })],
      credentials: [],
      credentialHealth: [],
    }).blocking).toBe(true);
  });

  it('returns the concrete credential failure instead of a generic auth status', () => {
    expect(getQueryAuthState({
      query: query({ site_id: 'source' }),
      sites: [authSite()],
      credentials: [{ site_category: 'source', credential_type: 'cookies', display_name: null, created_at: '' }],
      credentialHealth: [{ site_category: 'source', health_status: 'error', last_checked_at: '', last_error: 'Session cookie was rejected' }],
    })).toEqual({ tone: 'attention', label: 'Session cookie was rejected', blocking: true });
  });
});
