import { describe, expect, it, vi } from 'vitest';
import {
  createSubscriptionNotificationService,
  showSubscriptionSettlementNotification,
} from './subscriptionNotificationService.mjs';

function notificationMock() {
  const show = vi.fn();
  return {
    Notification: Object.assign(vi.fn(() => ({ show })), { isSupported: () => true }),
    show,
  };
}

describe('subscription OS notifications', () => {
  it.each(['failed', 'missing'])('retains a completed batch when an activity is %s and retries on the next refresh', async (failure) => {
    const { Notification, show } = notificationMock();
    const warning = vi.spyOn(console, 'warn').mockImplementation(() => {});
    let active = true;
    let unavailable = true;
    let activityReads = 0;
    const service = createSubscriptionNotificationService({
      Notification, app: {}, getCurrentLibraryRoot: () => '/library', platform: 'win32',
      invokeSerialized: async (command) => {
        if (command === 'subscriptions.list') return JSON.stringify({ subscriptions: active ? [{ active_run_id: 10, status: 'running' }] : [] });
        activityReads += 1;
        if (unavailable && failure === 'failed') throw new Error('fixture read failed');
        return JSON.stringify(unavailable ? null : { new_library_item_keys: ['new-collection'] });
      },
    });
    try {
      await service.refresh();
      active = false;
      await service.refresh();
      expect(show).not.toHaveBeenCalled();
      expect(activityReads).toBe(1);
      unavailable = false;
      await service.refresh();
      expect(activityReads).toBe(2);
      expect(Notification).toHaveBeenCalledWith({ title: 'Subscriptions finished', body: '1 new library item is ready to review.' });
      await service.refresh();
      expect(show).toHaveBeenCalledOnce();
    } finally {
      service.stop();
      warning.mockRestore();
    }
  });

  it.each(['list', 'activity'])('discards completion data when the library changes during the %s read', async (phase) => {
    const { Notification, show } = notificationMock();
    let root = '/library-a';
    let active = true;
    let pending = null;
    let release;
    const service = createSubscriptionNotificationService({
      Notification, app: {}, getCurrentLibraryRoot: () => root, platform: 'win32',
      invokeSerialized: async (command) => {
        if (!active && ((phase === 'list' && command === 'subscriptions.list')
          || (phase === 'activity' && command === 'subscriptions.runs.get'))) {
          pending = command;
          await new Promise((resolve) => { release = resolve; });
        }
        return JSON.stringify(command === 'subscriptions.list'
          ? { subscriptions: active ? [{ active_run_id: 10, status: 'running' }] : [] }
          : { new_library_item_keys: ['wrong-library-item'] });
      },
    });
    await service.refresh();
    active = false;
    const refresh = service.refresh();
    await vi.waitFor(() => expect(pending).not.toBeNull());
    root = '/library-b';
    release();
    await refresh;
    expect(show).not.toHaveBeenCalled();
    service.stop();
  });

  it('notifies once after every active run in the batch has settled', async () => {
    const { Notification, show } = notificationMock();
    let subscriptions = [
      { active_run_id: 10, status: 'running' },
      { active_run_id: 11, status: 'pending' },
    ];
    const invokeSerialized = vi.fn(async (command, args) => JSON.stringify(
      command === 'subscriptions.list'
        ? { subscriptions }
        : { summary: { counts: { ingested: 1169 } },
          new_library_item_keys: args.run_id === 10 ? ['collection-a', 'standalone-b'] : ['collection-a', 'collection-c'] },
    ));
    const service = createSubscriptionNotificationService({
      Notification,
      app: {},
      invokeSerialized,
      getCurrentLibraryRoot: () => '/library',
      platform: 'win32',
    });

    await service.refresh();
    subscriptions = [{ active_run_id: 11, status: 'running' }];
    await service.refresh();
    expect(show).not.toHaveBeenCalled();
    subscriptions = [];
    await service.refresh();

    expect(Notification).toHaveBeenCalledWith({
      title: 'Subscriptions finished',
      body: '3 new library items are ready to review.',
    });
    expect(show).toHaveBeenCalledOnce();
  });
});
