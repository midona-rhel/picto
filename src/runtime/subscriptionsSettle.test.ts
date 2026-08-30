import { afterEach, describe, expect, it, vi } from 'vitest';
import { getDefaultStore } from 'jotai';
import type { SubscriptionProgressEvent } from '../shared/types/subscriptions';

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
const cleanupGalleryImport = vi.hoisted(() => vi.fn());
const showErrorNotification = vi.hoisted(() => vi.fn());
const showSuccessNotification = vi.hoisted(() => vi.fn());

vi.mock('../controllers/subscriptionsController', () => ({
  subscriptionsController: {
    loadWorkspaceSnapshot,
    refreshRuntimeState,
    getRunActivity,
    listRuns,
    delete: deleteSubscription,
    cleanupGalleryImport,
  },
}));
vi.mock('../shared/lib/notifications', () => ({ showErrorNotification, showSuccessNotification }));

import {
  refreshSubscriptionsRuntimeState,
  refreshSubscriptionsWorkspace,
  retainGalleryProgressTotals,
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
    vi.useRealTimers();
    callbacks.clear();
    register.mockClear();
    loadWorkspaceSnapshot.mockClear();
    refreshRuntimeState.mockClear();
    getRunActivity.mockReset();
    listRuns.mockReset();
    deleteSubscription.mockReset();
    cleanupGalleryImport.mockReset();
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

  it('retains a known gallery total for the same run across partial polls', () => {
    const base = {
      subscription_id: '9',
      subscription_name: 'Gallery',
      run_id: 90,
      files_downloaded: 2,
    } as SubscriptionProgressEvent;
    const [settled] = retainGalleryProgressTotals(
      [{ ...base, gallery_total_items: 24 }],
      [{ ...base, files_downloaded: 3, gallery_total_items: null }],
    );

    expect(settled.gallery_total_items).toBe(24);
    expect(settled.files_downloaded).toBe(3);
  });

  it('refreshes persisted workspace state for subscription invalidation', async () => {
    vi.useFakeTimers();
    const stop = startSubscriptionsSettle();
    callbacks.get('subscriptions')?.();
    await vi.advanceTimersByTimeAsync(250);

    expect(loadWorkspaceSnapshot).toHaveBeenCalledOnce();
    stop();
  });

  it('coalesces a burst of subscription invalidations into one workspace read', async () => {
    vi.useFakeTimers();
    const stop = startSubscriptionsSettle();

    callbacks.get('subscriptions')?.();
    callbacks.get('subscriptions')?.();
    callbacks.get('subscriptions')?.();
    await vi.advanceTimersByTimeAsync(250);

    expect(loadWorkspaceSnapshot).toHaveBeenCalledOnce();
    stop();
  });

  it('runs a trailing refresh when invalidation races a manual run refresh', async () => {
    let releaseFirst = () => {};
    loadWorkspaceSnapshot
      .mockImplementationOnce(() => new Promise((resolve) => {
        releaseFirst = () => resolve({
          subscriptions: [],
          sites: [],
          credentials: [],
          credentialHealth: [],
          runningSubscriptionIds: [],
          runningProgress: [],
          listMetrics: {},
          covers: new Map(),
        });
      }))
      .mockResolvedValueOnce({
        subscriptions: [{ id: '7', name: 'Active feed' }],
        sites: [],
        credentials: [],
        credentialHealth: [],
        runningSubscriptionIds: ['7'],
        runningProgress: [],
        listMetrics: {},
        covers: new Map(),
      });

    const first = refreshSubscriptionsWorkspace();
    const trailing = refreshSubscriptionsWorkspace();
    releaseFirst();
    await Promise.all([first, trailing]);

    expect(loadWorkspaceSnapshot).toHaveBeenCalledTimes(2);
    expect(store.get(subscriptionsWorkspaceSnapshotAtom)?.runningSubscriptionIds).toEqual(['7']);
  });

  it('does not let an older runtime read overwrite a newer workspace snapshot', async () => {
    const original = { id: '7', name: 'Active feed' };
    const refreshed = { id: '7', name: 'Renamed feed' };
    store.set(subscriptionsWorkspaceSnapshotAtom, {
      subscriptions: [original],
      sites: [],
      credentials: [],
      credentialHealth: [],
      runningSubscriptionIds: ['7'],
      runningProgress: [{ subscription_id: '7', subscription_name: original.name, run_id: 44 }],
      listMetrics: {},
    } as never);
    getRunActivity.mockResolvedValue(runActivity('running', 'running'));

    let releaseRuntime = () => {};
    refreshRuntimeState.mockImplementationOnce(() => new Promise((resolve) => {
      releaseRuntime = () => resolve({ runningSubscriptionIds: [], runningProgress: [] });
    }));
    loadWorkspaceSnapshot.mockResolvedValueOnce({
      subscriptions: [refreshed],
      sites: [],
      credentials: [],
      credentialHealth: [],
      runningSubscriptionIds: ['7'],
      runningProgress: [{ subscription_id: '7', subscription_name: refreshed.name, run_id: 44 }],
      listMetrics: {},
      covers: new Map(),
    });

    const staleRuntimeRead = refreshSubscriptionsRuntimeState();
    await refreshSubscriptionsWorkspace();
    releaseRuntime();
    await staleRuntimeRead;

    const snapshot = store.get(subscriptionsWorkspaceSnapshotAtom);
    expect(snapshot?.subscriptions[0]?.name).toBe('Renamed feed');
    expect(snapshot?.runningSubscriptionIds).toEqual(['7']);
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
    refreshRuntimeState
      .mockResolvedValueOnce({
        runningSubscriptionIds: ['7'],
        runningProgress: [{ ...progress, phase: 'downloading', status_text: 'running' }],
      })
      .mockResolvedValueOnce({
        runningSubscriptionIds: ['7'],
        runningProgress: [{ ...progress, phase: 'settling', status_text: 'query completed' }],
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
      message: 'Active feed · artist-name · 12 posts added to library',
    }));
  });

  it('does not reread run activity while reported run state is unchanged', async () => {
    const subscription = { id: '7', name: 'Active feed' };
    const progress = {
      subscription_id: '7',
      subscription_name: 'Active feed',
      run_id: 44,
      phase: 'downloading',
      status_text: 'running',
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
    getRunActivity.mockResolvedValue(runActivity('running', 'running'));

    await refreshSubscriptionsRuntimeState();
    await vi.waitFor(() => expect(getRunActivity).toHaveBeenCalledOnce());
    await refreshSubscriptionsRuntimeState();

    expect(getRunActivity).toHaveBeenCalledOnce();
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
      message: 'Active feed · 1 query completed · 12 posts added to library',
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
    cleanupGalleryImport.mockResolvedValue({ title: 'Example Gallery', already_exists: false });

    await refreshSubscriptionsRuntimeState();
    await vi.waitFor(() => expect(cleanupGalleryImport).toHaveBeenCalledWith('9'));

    expect(showSuccessNotification).toHaveBeenCalledWith({
      title: 'Gallery downloaded',
      message: 'Example Gallery has been downloaded.',
    });
    expect(store.get(subscriptionsWorkspaceSnapshotAtom)?.subscriptions).toEqual([]);
    expect(getRunActivity).not.toHaveBeenCalled();
  });

  it('reports an all-known gallery without curating another collection', async () => {
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
      posts_added: 0,
      posts_skipped: 1,
      error_message: null,
    }]);
    cleanupGalleryImport.mockResolvedValue({ title: 'Example Gallery', already_exists: true });

    await refreshSubscriptionsRuntimeState();
    await vi.waitFor(() => expect(cleanupGalleryImport).toHaveBeenCalledWith('9'));

    expect(showSuccessNotification).toHaveBeenCalledWith({
      title: 'Gallery already exists',
      message: 'Example Gallery is already in the library.',
    });
  });

  it('keeps a queued gallery import until its run record exists', async () => {
    const galleryJob = {
      id: '9',
      name: 'ExHentai Gallery 1169267',
      queries: [{ site_id: 'ehentai' }],
    };
    store.set(subscriptionsWorkspaceSnapshotAtom, {
      subscriptions: [galleryJob],
      runningSubscriptionIds: [],
      runningProgress: [],
    } as never);
    refreshRuntimeState.mockResolvedValue({ runningSubscriptionIds: [], runningProgress: [] });
    listRuns.mockResolvedValue([]);

    await refreshSubscriptionsRuntimeState();
    await vi.waitFor(() => expect(listRuns).toHaveBeenCalledWith('9'));

    expect(cleanupGalleryImport).not.toHaveBeenCalled();
    expect(showErrorNotification).not.toHaveBeenCalled();
    expect(store.get(subscriptionsWorkspaceSnapshotAtom)?.subscriptions).toEqual([galleryJob]);
  });

  it('keeps a gallery import while its persisted run is pending', async () => {
    const galleryJob = {
      id: '9',
      name: 'E-Hentai Gallery 12345',
      queries: [{ site_id: 'ehentai' }],
    };
    store.set(subscriptionsWorkspaceSnapshotAtom, {
      subscriptions: [galleryJob],
      runningSubscriptionIds: [],
      runningProgress: [],
    } as never);
    refreshRuntimeState.mockResolvedValue({ runningSubscriptionIds: [], runningProgress: [] });
    listRuns.mockResolvedValue([{
      status: 'pending',
      failure_kind: null,
      error_message: null,
    }]);

    await refreshSubscriptionsRuntimeState();
    await vi.waitFor(() => expect(listRuns).toHaveBeenCalledWith('9'));

    expect(cleanupGalleryImport).not.toHaveBeenCalled();
    expect(showErrorNotification).not.toHaveBeenCalled();
    expect(store.get(subscriptionsWorkspaceSnapshotAtom)?.subscriptions).toEqual([galleryJob]);
  });

  it('shows the gallery source failure before cleaning up the transient job', async () => {
    const galleryJob = {
      id: '9',
      name: 'ExHentai Gallery 1169267',
      queries: [{ site_id: 'ehentai' }],
    };
    store.set(subscriptionsWorkspaceSnapshotAtom, {
      subscriptions: [galleryJob],
      runningSubscriptionIds: [],
      runningProgress: [],
    } as never);
    refreshRuntimeState.mockResolvedValue({ runningSubscriptionIds: [], runningProgress: [] });
    listRuns.mockResolvedValue([{
      status: 'failed',
      failure_kind: 'authentication',
      error_message: 'AuthorizationError: ExHentai rejected the saved cookies',
    }]);
    cleanupGalleryImport.mockResolvedValue(null);

    await refreshSubscriptionsRuntimeState();
    await vi.waitFor(() => expect(cleanupGalleryImport).toHaveBeenCalledWith('9'));

    expect(showErrorNotification).toHaveBeenCalledWith({
      title: 'Gallery import failed',
      message: 'AuthorizationError: ExHentai rejected the saved cookies',
    });
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
