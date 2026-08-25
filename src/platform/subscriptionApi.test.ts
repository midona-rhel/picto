import { beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.hoisted(() => vi.fn());
vi.mock('./ipc', () => ({ invoke }));

import {
  addSubscriptionQuery,
  createSubscription,
  getSubscriptionCoverCandidates,
  getSubscriptionCovers,
  getRunningSubscriptions,
  getRunningSubscriptionProgress,
  getSubscriptions,
  listCredentialHealth,
  listCredentials,
  listSubscriptionIssues,
  listSubscriptionRuns,
  pauseSubscriptionQuery,
  runSubscription,
  setSubscriptionDestination,
  setSubscriptionCover,
  setSubscriptionPostsPerRun,
  stopSubscription,
} from './subscriptionApi';

const replacementSubscription = {
  subscription_id: 7,
  name: 'Artists',
  schedule: 'daily',
  paused: false,
  initial_post_limit: 100,
  periodic_post_limit: 20,
  next_run_at: null,
  status: 'running',
  active_run_id: 11,
  media_count: 3,
  cover_file_hash: 'cover-hash',
  cover_focus_x: 250,
  cover_focus_y: 750,
  cover_zoom_percent: 160,
  destination: { target_folder_ids: [], automatic_tags: [] },
  progress: { discovered: 5, downloaded: 4, ingested: 3, failed: 0, deleted: 0 },
  queries: [{
    query_id: 9,
    site_id: 'gelbooru',
    query_kind: 'tag',
    query_text: 'artist',
    display_name: null,
    notes: null,
    paused: false,
    initial_run_complete: true,
    source_history_complete: true,
    successful_run_count: 1,
    last_success_at: '2026-08-23T10:00:00Z',
    last_failure_at: null,
    last_failure_kind: null,
    last_failure_message: null,
    post_count: 3,
    media_count: 3,
  }],
};

function listPayload(overrides = {}) {
  return { subscriptions: [{ ...replacementSubscription, ...overrides }], revision: 4 };
}

describe('replacement subscription API', () => {
  beforeEach(() => invoke.mockReset());

  it('reads the replacement catalog and maps numeric IDs at the UI boundary', async () => {
    invoke.mockResolvedValue(listPayload());

    await expect(getSubscriptions()).resolves.toEqual([expect.objectContaining({
      id: '7',
      total_files: 3,
      queries: [expect.objectContaining({ id: '9', files_found: 3 })],
    })]);
    await expect(getRunningSubscriptions()).resolves.toEqual(['7']);
    expect(invoke.mock.calls.every(([command]) => command === 'subscriptions.list')).toBe(true);
  });

  it('uses persisted replacement commands for run, cancel, and query pause', async () => {
    invoke.mockResolvedValue({ revision: 5, resources: ['subscriptions'], item_ids: [] });

    await runSubscription('7');
    await stopSubscription('7');
    await pauseSubscriptionQuery('9', true);

    expect(invoke.mock.calls).toEqual([
      ['subscriptions.run', { subscription_id: 7 }],
      ['subscriptions.cancel', { subscription_id: 7 }],
      ['subscriptions.queries.pause', { query_id: 9, paused: true }],
    ]);
  });

  it('persists the destination folder and automatic tags through one command', async () => {
    invoke.mockResolvedValue({ revision: 5, resources: ['subscriptions'], item_ids: [] });

    await setSubscriptionDestination('7', {
      target_folder_ids: [42, 43],
      automatic_tags: ['creator:alice', 'favorite'],
    });

    expect(invoke).toHaveBeenCalledWith('subscriptions.destination', {
      subscription_id: 7,
      destination: {
        target_folder_ids: [42, 43],
        automatic_tags: ['creator:alice', 'favorite'],
      },
    });
  });

  it('persists one posts-per-run value at the subscription boundary', async () => {
    invoke.mockResolvedValue({ revision: 5, resources: ['subscriptions'], item_ids: [] });

    await setSubscriptionPostsPerRun('7', 25);

    expect(invoke).toHaveBeenCalledWith('subscriptions.posts_per_run', {
      subscription_id: 7,
      posts_per_run: 25,
    });
  });

  it('reads and writes a cropped cover through the replacement contract', async () => {
    invoke
      .mockResolvedValueOnce(listPayload())
      .mockResolvedValueOnce({
        candidates: [{ media_item_id: 8, file_hash: 'cover-hash', name: 'Cover' }],
        next_cursor: { imported_at: '2026-08-23T10:00:00Z', media_item_id: 8 },
      })
      .mockResolvedValueOnce({ revision: 5, resources: ['subscriptions'], item_ids: [] });

    await expect(getSubscriptionCovers()).resolves.toEqual(new Map([['7', {
      file_hash: 'cover-hash',
      focus_x: 250,
      focus_y: 750,
      zoom_percent: 160,
    }]]));
    await expect(getSubscriptionCoverCandidates('7')).resolves.toEqual({
      candidates: [expect.objectContaining({ media_item_id: 8, file_hash: 'cover-hash' })],
      next_cursor: { imported_at: '2026-08-23T10:00:00Z', media_item_id: 8 },
    });
    await setSubscriptionCover('7', {
      media_item_id: 8,
      focus_x: 250,
      focus_y: 750,
      zoom_percent: 160,
    });

    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      'subscriptions.list',
      'subscriptions.cover.candidates',
      'subscriptions.cover.set',
    ]);
    expect(invoke).toHaveBeenCalledWith('subscriptions.cover.candidates', {
      subscription_id: 7,
      cursor: null,
      limit: 200,
    });
  });

  it('passes the stable cover cursor and bounded page size to the backend', async () => {
    invoke.mockResolvedValue({ candidates: [], next_cursor: null });
    const cursor = { imported_at: '2026-08-23T10:00:00Z', media_item_id: 8 };

    await getSubscriptionCoverCandidates('7', cursor, 50);

    expect(invoke).toHaveBeenCalledWith('subscriptions.cover.candidates', {
      subscription_id: 7,
      cursor,
      limit: 50,
    });
  });

  it('reads active progress from the persisted replacement progress command', async () => {
    invoke
      .mockResolvedValueOnce(listPayload())
      .mockResolvedValueOnce({
        subscription_id: 7,
        run_id: 11,
        status: 'running',
        counts: { posts_traversed: 6, posts_added: 4, fetched: 8, downloaded: 7, queued: 2, ingested: 5, failed: 1, deleted: 0 },
      });

    await expect(getRunningSubscriptionProgress()).resolves.toEqual([
      expect.objectContaining({
        files_downloaded: 7,
        media_added: 5,
        failed_ingest: 1,
        posts_traversed: 6,
        posts_added: 4,
      }),
    ]);
    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      'subscriptions.list',
      'subscriptions.progress.get',
    ]);
  });

  it('creates a subscription and reads its canonical persisted view back', async () => {
    invoke
      .mockResolvedValueOnce({ subscription_id: 7, receipt: { revision: 1, resources: [], item_ids: [] } })
      .mockResolvedValueOnce(listPayload());

    await expect(createSubscription({ name: 'Artists' })).resolves.toEqual(expect.objectContaining({ id: '7' }));
    expect(invoke.mock.calls[0]).toEqual(['subscriptions.create', {
      name: 'Artists',
      schedule: 'manual',
      initial_post_limit: 100,
      periodic_post_limit: 100,
      queries: [],
    }]);
  });

  it('adds a query through the replacement query command and reads it back', async () => {
    invoke
      .mockResolvedValueOnce({ query_id: 9, receipt: { revision: 1, resources: [], item_ids: [] } })
      .mockResolvedValueOnce(listPayload());

    await expect(addSubscriptionQuery('7', 'gelbooru', 'artist')).resolves.toEqual(
      expect.objectContaining({ id: '9', query_text: 'artist' }),
    );
    expect(invoke.mock.calls[0][0]).toBe('subscriptions.queries.add');
  });

  it('maps persisted run and issue pages without legacy attempt reads', async () => {
    invoke
      .mockResolvedValueOnce({
        subscription_id: 7,
        runs: [{
          run_id: 11,
          subscription_id: 7,
          requested_by: 'manual',
          status: 'succeeded',
          started_at: '2026-08-23T10:00:00Z',
          finished_at: '2026-08-23T10:01:00Z',
          failure_kind: null,
          error_message: null,
          created_at: '2026-08-23T10:00:00Z',
          query_count: 1,
          counts: { posts_traversed: 7, posts_added: 2, fetched: 3, downloaded: 3, queued: 0, ingested: 3, failed: 0, deleted: 0 },
        }],
      })
      .mockResolvedValueOnce({ subscription_id: 7, issues: [{
        issue_id: 1,
        issue_key: 'login',
        subscription_id: 7,
        query_id: 9,
        issue_kind: 'credential_blocked',
        message: 'Login required',
        detail: null,
        status: 'open',
        first_seen_at: '2026-08-23T10:00:00Z',
        last_seen_at: '2026-08-23T10:00:00Z',
        resolved_at: null,
      }], next_cursor: null, total_count: 1 });

    await expect(listSubscriptionRuns('7')).resolves.toEqual([expect.objectContaining({
      run_id: 11,
      posts_traversed: 7,
      posts_added: 2,
      media_added: 3,
      files_downloaded: 3,
    })]);
    await expect(listSubscriptionIssues('7')).resolves.toEqual({
      items: [expect.objectContaining({ issue_id: 1, recovery_action: 'fix_credentials' })],
      next_cursor: null,
      total_count: 1,
    });
    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      'subscriptions.runs.list',
      'subscriptions.issues.list',
    ]);
  });

  it('reads credential status without exposing credential writes to the renderer', async () => {
    invoke
      .mockResolvedValueOnce([{ site_id: 'pixiv', credential_type: 'cookies', display_name: 'Pixiv', created_at: 'now' }])
      .mockResolvedValueOnce([{ site_id: 'pixiv', status: 'valid', checked_at: 'now', last_error: null }]);

    await listCredentials();
    await listCredentialHealth();

    expect(invoke.mock.calls).toEqual([
      ['auth.credentials.list', {}],
      ['auth.health.list', {}],
    ]);
  });
});
