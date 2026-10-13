import { describe, expect, it, vi } from 'vitest';
import {
  createSubscriptionNotificationService,
  installDockBadgeClearing,
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

  it('caps the macOS dock badge at 9,999', () => {
    const { Notification } = notificationMock();
    const setBadge = vi.fn();
    showSubscriptionSettlementNotification({
      Notification,
      app: { dock: { setBadge } },
      platform: 'darwin',
      newItems: 20_000,
    });
    expect(setBadge).toHaveBeenCalledWith('9999');
  });
});

describe('dock badge', () => {
  function dockApp() {
    const listeners = new Map();
    return {
      dock: { setBadge: vi.fn() },
      on: (event, listener) => listeners.set(event, listener),
      emit: (event) => listeners.get(event)?.(),
    };
  }

  it('is set for items that arrive in the background and cleared on focus or activate', () => {
    const { Notification } = notificationMock();
    const app = dockApp();
    installDockBadgeClearing({ app, platform: 'darwin' });
    showSubscriptionSettlementNotification({ Notification, app, platform: 'darwin', newItems: 100 });
    expect(app.dock.setBadge).toHaveBeenLastCalledWith('100');
    app.emit('browser-window-focus');
    expect(app.dock.setBadge).toHaveBeenLastCalledWith('');
    showSubscriptionSettlementNotification({ Notification, app, platform: 'darwin', newItems: 3 });
    app.emit('activate');
    expect(app.dock.setBadge).toHaveBeenLastCalledWith('');
  });

  it('is not set while a Picto window has focus', () => {
    const { Notification, show } = notificationMock();
    const app = dockApp();
    showSubscriptionSettlementNotification({ Notification, app, platform: 'darwin', newItems: 5, appFocused: true });
    expect(app.dock.setBadge).not.toHaveBeenCalled();
    expect(show).toHaveBeenCalledOnce();
  });

  it('does nothing off macOS', () => {
    const app = dockApp();
    installDockBadgeClearing({ app, platform: 'win32' });
    app.emit('browser-window-focus');
    expect(app.dock.setBadge).not.toHaveBeenCalled();
  });
});
