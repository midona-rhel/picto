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
  it('notifies once after every active run in the batch has settled', async () => {
    const { Notification, show } = notificationMock();
    let subscriptions = [
      { active_run_id: 10, status: 'running' },
      { active_run_id: 11, status: 'pending' },
    ];
    const invokeSerialized = vi.fn(async (command, args) => JSON.stringify(
      command === 'subscriptions.list'
        ? { subscriptions }
        : { summary: { counts: { ingested: args.run_id === 10 ? 3 : 4 } } },
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
      body: '7 new images are ready to review.',
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
