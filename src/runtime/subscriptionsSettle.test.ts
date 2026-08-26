import { afterEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';

const { callbacks, register } = vi.hoisted(() => {
  const callbacks = new Map<string, () => void>();
  const register = vi.fn((resource: string, callback: () => void) => {
    callbacks.set(resource, callback);
    return () => callbacks.delete(resource);
  });
  return { callbacks, register };
});

vi.mock('./libraryInvalidation', () => ({
  libraryInvalidation: { register },
}));

const loadWorkspaceSnapshot = vi.hoisted(() => vi.fn().mockResolvedValue({
  subscriptions: [],
  sites: [],
  credentials: [],
  credentialHealth: [],
  runningSubscriptionIds: [],
  runningProgress: [],
  listMetrics: {},
  covers: new Map(),
}));
const refreshRuntimeState = vi.hoisted(() => vi.fn().mockResolvedValue({
  runningSubscriptionIds: [],
  runningProgress: [],
}));
const getRunActivity = vi.hoisted(() => vi.fn());
const listRuns = vi.hoisted(() => vi.fn());
const deleteSubscription = vi.hoisted(() => vi.fn());
const showErrorNotification = vi.hoisted(() => vi.fn());
const showSuccessNotification = vi.hoisted(() => vi.fn());

vi.mock('../controllers/subscriptionsController', () => ({
  subscriptionsController: {
    loadWorkspaceSnapshot,
    refreshRuntimeState,
    getRunActivity,
    listRuns,
    delete: deleteSubscription,
  },
}));
vi.mock('../shared/lib/notifications', () => ({ showErrorNotification, showSuccessNotification }));

import {
  refreshSubscriptionsRuntimeState,
  refreshSubscriptionsWorkspace,
  resetSubscriptionsSettleForTests,
  startSubscriptionsSettle,
} from './subscriptionsSettle';
import {
  subscriptionsCoversAtom,
  subscriptionsWorkspaceSnapshotAtom,
} from '../state/subscriptionsWorkspace';

const store = getDefaultStore();

describe('subscription settlement', () => {
  afterEach(() => {
    callbacks.clear();
    register.mockClear();
    loadWorkspaceSnapshot.mockClear();
    refreshRuntimeState.mockClear();
    getRunActivity.mockReset();
    listRuns.mockReset();
    deleteSubscription.mockReset();
    showErrorNotification.mockClear();
    showSuccessNotification.mockClear();
    resetSubscriptionsSettleForTests();
    store.set(subscriptionsCoversAtom, new Map());
    store.set(subscriptionsWorkspaceSnapshotAtom, null);
  });

  it('uses replacement resources and does not subscribe to legacy runtime events', () => {
    const stop = startSubscriptionsSettle();

    expect([...callbacks.keys()]).toEqual(['subscriptions', 'tasks']);
    expect(register).toHaveBeenCalledTimes(2);

    stop();
    expect(callbacks.size).toBe(0);
  });

  it('refreshes persisted workspace state for subscription invalidation', async () => {
    const stop = startSubscriptionsSettle();
    callbacks.get('subscriptions')?.();
    await Promise.resolve();

    expect(loadWorkspaceSnapshot).toHaveBeenCalledOnce();
    stop();
  });

  it('refreshes persisted task progress for task invalidation', async () => {
    const runningSubscription = { id: '7', name: 'Active feed' };
    store.set(subscriptionsWorkspaceSnapshotAtom, {
      subscriptions: [runningSubscription],
      sites: [],
      credentials: [],
      credentialHealth: [],
      runningSubscriptionIds: ['7'],
      runningProgress: [],
      listMetrics: {},
    } as never);
    const stop = startSubscriptionsSettle();
    callbacks.get('tasks')?.();
    await Promise.resolve();

    expect(refreshRuntimeState).toHaveBeenCalledWith([runningSubscription]);
    stop();
  });

  it('reports workspace failures through the shared notification path', async () => {
    loadWorkspaceSnapshot.mockRejectedValueOnce(new Error('backend unavailable'));

    await refreshSubscriptionsWorkspace();

    expect(showErrorNotification).toHaveBeenCalledWith({
      title: 'Subscriptions unavailable',
      message: 'backend unavailable',
    });
  });

  it('holds the cover stable while a run is active and updates it when the run finishes', async () => {
    const oldCover = { file_hash: 'old', focus_x: 500, focus_y: 500, zoom_percent: 100 };
    const newCover = { file_hash: 'new', focus_x: 500, focus_y: 500, zoom_percent: 100 };
    store.set(subscriptionsCoversAtom, new Map([['1', oldCover]]));
    loadWorkspaceSnapshot.mockResolvedValueOnce({
      subscriptions: [],
      sites: [],
      credentials: [],
      credentialHealth: [],
      runningSubscriptionIds: ['1'],
      runningProgress: [],
      listMetrics: {},
      covers: new Map([['1', newCover]]),
    });

    await refreshSubscriptionsWorkspace();
    expect(store.get(subscriptionsCoversAtom).get('1')).toEqual(oldCover);

    loadWorkspaceSnapshot.mockResolvedValueOnce({
      subscriptions: [],
      sites: [],
      credentials: [],
      credentialHealth: [],
      runningSubscriptionIds: [],
      runningProgress: [],
      listMetrics: {},
      covers: new Map([['1', newCover]]),
    });

    await refreshSubscriptionsWorkspace();
    expect(store.get(subscriptionsCoversAtom).get('1')).toEqual(newCover);
  });

  it('notifies when a persisted query crosses into a successful terminal state', async () => {
    const subscription = { id: '7', name: 'Active feed' };
    const progress = {
      subscription_id: '7',
      subscription_name: 'Active feed',
      run_id: 44,
    };
    store.set(subscriptionsWorkspaceSnapshotAtom, {
      subscriptions: [subscription],
      runningSubscriptionIds: ['7'],
      runningProgress: [progress],
    } as never);
    refreshRuntimeState.mockResolvedValue({
      runningSubscriptionIds: ['7'],
      runningProgress: [progress],
    });
    getRunActivity
      .mockResolvedValueOnce(runActivity('running', 'running'))
      .mockResolvedValueOnce(runActivity('running', 'succeeded'));

    await refreshSubscriptionsRuntimeState();
    await vi.waitFor(() => expect(getRunActivity).toHaveBeenCalledTimes(1));
    expect(showSuccessNotification).not.toHaveBeenCalled();

    await refreshSubscriptionsRuntimeState();
    await vi.waitFor(() => expect(showSuccessNotification).toHaveBeenCalledWith({
      title: 'Query completed',
      message: 'Active feed · artist-name · 100 posts traversed · 12 media added',
    }));
  });

  it('notifies when a persisted subscription run completes', async () => {
    const subscription = { id: '7', name: 'Active feed' };
    const progress = {
      subscription_id: '7',
      subscription_name: 'Active feed',
      run_id: 44,
    };
    store.set(subscriptionsWorkspaceSnapshotAtom, {
      subscriptions: [subscription],
      runningSubscriptionIds: ['7'],
      runningProgress: [progress],
    } as never);
    refreshRuntimeState.mockResolvedValue({
      runningSubscriptionIds: [],
      runningProgress: [],
    });
    getRunActivity.mockResolvedValue(runActivity('succeeded', 'succeeded'));

    await refreshSubscriptionsRuntimeState();
    await vi.waitFor(() => expect(showSuccessNotification).toHaveBeenCalledWith({
      title: 'Subscription completed',
      message: 'Active feed · 1 query completed · 100 posts traversed · 12 media added',
    }));
  });

  it('removes a completed gallery import without treating it as a subscription', async () => {
    const galleryJob = {
      id: '9',
      name: 'E-Hentai Gallery 12345',
      queries: [{ site_id: 'ehentai' }],
    };
    store.set(subscriptionsWorkspaceSnapshotAtom, {
      subscriptions: [galleryJob],
      runningSubscriptionIds: ['9'],
      runningProgress: [{
        subscription_id: '9',
        subscription_name: galleryJob.name,
        run_id: 90,
      }],
    } as never);
    refreshRuntimeState.mockResolvedValue({ runningSubscriptionIds: [], runningProgress: [] });
    listRuns.mockResolvedValue([{
      status: 'succeeded',
      media_added: 24,
      error_message: null,
    }]);
    deleteSubscription.mockResolvedValue(undefined);

    await refreshSubscriptionsRuntimeState();
    await vi.waitFor(() => expect(deleteSubscription).toHaveBeenCalledWith('9'));

    expect(showSuccessNotification).toHaveBeenCalledWith({
      title: 'Gallery imported',
      message: '24 media added',
    });
    expect(store.get(subscriptionsWorkspaceSnapshotAtom)?.subscriptions).toEqual([]);
    expect(getRunActivity).not.toHaveBeenCalled();
  });
});

function runActivity(runStatus: string, queryStatus: string) {
  const counts = {
    posts_traversed: 100,
    posts_added: 12,
    fetched: 12,
    downloaded: 12,
    queued: 12,
    ingested: 12,
    failed: 0,
    deleted: 0,
  };
  return {
    summary: {
      run_id: 44,
      subscription_id: 7,
      requested_by: 'manual',
      status: runStatus,
      started_at: null,
      finished_at: null,
      failure_kind: null,
      error_message: null,
      created_at: '2026-08-25T00:00:00Z',
      query_count: 1,
      counts,
    },
    queries: [{
      run_query_id: 45,
      run_id: 44,
      query_id: 46,
      site_id: 'pixiv',
      query_text: 'artist-name',
      status: queryStatus,
      attempt_count: 1,
      started_at: null,
      finished_at: null,
      failure_kind: null,
      error_message: null,
      counts,
      source_items: [],
      source_items_truncated: false,
    }],
  };
}
