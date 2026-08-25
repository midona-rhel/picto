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
const showErrorNotification = vi.hoisted(() => vi.fn());

vi.mock('../controllers/subscriptionsController', () => ({
  subscriptionsController: { loadWorkspaceSnapshot, refreshRuntimeState },
}));
vi.mock('../shared/lib/notifications', () => ({ showErrorNotification }));

import { refreshSubscriptionsWorkspace, startSubscriptionsSettle } from './subscriptionsSettle';
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
    showErrorNotification.mockClear();
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
});
