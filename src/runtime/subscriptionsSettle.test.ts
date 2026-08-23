import { afterEach, describe, expect, it, vi } from 'vitest';

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
}));
const refreshRuntimeState = vi.hoisted(() => vi.fn().mockResolvedValue({
  runningSubscriptionIds: [],
  runningProgress: [],
}));
const getCovers = vi.hoisted(() => vi.fn().mockResolvedValue(new Map()));

vi.mock('../controllers/subscriptionsController', () => ({
  subscriptionsController: { loadWorkspaceSnapshot, refreshRuntimeState, getCovers },
}));

import { startSubscriptionsSettle } from './subscriptionsSettle';

describe('subscription settlement', () => {
  afterEach(() => {
    callbacks.clear();
    register.mockClear();
    loadWorkspaceSnapshot.mockClear();
    refreshRuntimeState.mockClear();
    getCovers.mockClear();
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
    expect(getCovers).toHaveBeenCalledOnce();
    stop();
  });

  it('refreshes persisted task progress for task invalidation', async () => {
    const stop = startSubscriptionsSettle();
    callbacks.get('tasks')?.();
    await Promise.resolve();

    expect(refreshRuntimeState).toHaveBeenCalledOnce();
    stop();
  });
});
